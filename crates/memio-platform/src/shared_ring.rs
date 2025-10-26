//! Shared ring buffer implementation.
//!
//! Provides a memory-mapped file-based ring buffer for inter-process communication.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use memmap2::MmapMut;

use memio_core::{MemioError, MemioResult};

const RING_MAGIC: u64 = 0x5455_5242_4F52_494E; // "MEMIORIN"
static RING_COUNTER: AtomicU64 = AtomicU64::new(0);

#[repr(C)]
struct RingHeader {
    magic: u64,
    capacity: u64,
    head: AtomicU64,
    tail: AtomicU64,
}

/// A shared ring buffer backed by a memory-mapped file.
pub struct SharedRingBuffer {
    path: PathBuf,
    mmap: MmapMut,
    data_offset: usize,
    capacity: usize,
}

impl SharedRingBuffer {
    /// Creates a new ring buffer with the given capacity.
    pub fn create(capacity: usize) -> MemioResult<Self> {
        if capacity == 0 {
            return Err(MemioError::Internal("Ring capacity must be > 0".to_string()));
        }

        let mut path = PathBuf::from("/dev/shm");
        let id = std::process::id();
        let nonce = RING_COUNTER.fetch_add(1, Ordering::Relaxed);
        path.push(format!("memio_ring_{}_{}.bin", id, nonce));
        Self::open_or_create(path, capacity, true)
    }

    /// Opens an existing ring buffer from the given path.
    pub fn open(path: impl AsRef<Path>) -> MemioResult<Self> {
        let path = path.as_ref().to_path_buf();
        let metadata = std::fs::metadata(&path)?;
        let size = metadata.len() as usize;
        Self::open_or_create(path, size, false)
    }

    /// Returns the path to the ring buffer file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the data capacity of the ring buffer.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Writes data to the ring buffer.
    ///
    /// Returns the number of bytes written. May be less than `data.len()`
    /// if the buffer is full.
    pub fn write(&mut self, data: &[u8]) -> MemioResult<usize> {
        if data.is_empty() {
            return Ok(0);
        }

        let header_ptr = self.mmap.as_mut_ptr() as *mut RingHeader;
        let head = unsafe { (*header_ptr).head.load(Ordering::Acquire) };
        let tail = unsafe { (*header_ptr).tail.load(Ordering::Acquire) };
        let used = head.wrapping_sub(tail) as usize;

        if used >= self.capacity {
            return Ok(0);
        }

        let free = self.capacity - used;
        let to_write = data.len().min(free);
        if to_write == 0 {
            return Ok(0);
        }

        let write_pos = (head as usize) % self.capacity;
        let first = to_write.min(self.capacity - write_pos);
        let second = to_write - first;
        let base = unsafe { self.mmap.as_mut_ptr().add(self.data_offset) };

        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), base.add(write_pos), first);
            if second > 0 {
                std::ptr::copy_nonoverlapping(data.as_ptr().add(first), base, second);
            }
        }

        unsafe {
            (*header_ptr)
                .head
                .store(head.wrapping_add(to_write as u64), Ordering::Release);
        }
        Ok(to_write)
    }

    /// Reads data from the ring buffer.
    ///
    /// Returns the number of bytes read. May be less than `out.len()`
    /// if the buffer doesn't have enough data.
    pub fn read(&mut self, out: &mut [u8]) -> MemioResult<usize> {
        if out.is_empty() {
            return Ok(0);
        }

        let header_ptr = self.mmap.as_mut_ptr() as *mut RingHeader;
        let head = unsafe { (*header_ptr).head.load(Ordering::Acquire) };
        let tail = unsafe { (*header_ptr).tail.load(Ordering::Acquire) };
        let available = head.wrapping_sub(tail) as usize;

        if available == 0 {
            return Ok(0);
        }

        let to_read = out.len().min(available);
        let read_pos = (tail as usize) % self.capacity;
        let first = to_read.min(self.capacity - read_pos);
        let second = to_read - first;
        let base = unsafe { self.mmap.as_mut_ptr().add(self.data_offset) };

        unsafe {
            std::ptr::copy_nonoverlapping(base.add(read_pos), out.as_mut_ptr(), first);
            if second > 0 {
                std::ptr::copy_nonoverlapping(base, out.as_mut_ptr().add(first), second);
            }
        }

        unsafe {
            (*header_ptr)
                .tail
                .store(tail.wrapping_add(to_read as u64), Ordering::Release);
        }
        Ok(to_read)
    }

    fn open_or_create(path: PathBuf, capacity: usize, create: bool) -> MemioResult<Self> {
        let header_size = std::mem::size_of::<RingHeader>();
        let data_offset = align_up(header_size, 64);
        let file_len = data_offset + capacity;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .open(&path)?;

        if create {
            file.set_len(file_len as u64)?;
        }

        let mut mmap = unsafe { MmapMut::map_mut(&file)? };
        let header_ptr = mmap.as_mut_ptr() as *mut RingHeader;

        if create {
            unsafe {
                header_ptr.write(RingHeader {
                    magic: RING_MAGIC,
                    capacity: capacity as u64,
                    head: AtomicU64::new(0),
                    tail: AtomicU64::new(0),
                });
            }
        } else {
            let header = unsafe { &*header_ptr };
            if header.magic != RING_MAGIC {
                return Err(MemioError::Internal("Invalid ring buffer magic.".to_string()));
            }
        }

        Ok(Self {
            path,
            mmap,
            data_offset,
            capacity,
        })
    }

    #[allow(dead_code)]
    fn header(&self) -> &RingHeader {
        unsafe { &*(self.mmap.as_ptr() as *const RingHeader) }
    }
}

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}
