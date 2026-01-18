//! Android memio region implementation.
//!
//! Provides memio region functionality using the ASharedMemory API.

use std::collections::HashMap;
use std::os::unix::io::RawFd;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use once_cell::sync::Lazy;

use memio_core::{
    SHARED_STATE_HEADER_SIZE, SharedMemoryError, SharedMemoryFactory, SharedMemoryRegion,
    SharedStateInfo, read_header_ptr, write_header_ptr,
};

const HEADER_SIZE: usize = SHARED_STATE_HEADER_SIZE;

/// Counter for generating unique region names
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Registry of memio regions by name
static REGISTRY: Lazy<Mutex<HashMap<String, AndroidSharedMemoryRegion>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Android memio region using ASharedMemory.
#[derive(Debug)]
pub struct AndroidSharedMemoryRegion {
    name: String,
    #[cfg(target_os = "android")]
    fd: RawFd,
    ptr: *mut u8,
    size: usize,
    capacity: usize,
}

// SAFETY: The pointer is only accessed through synchronized methods
unsafe impl Send for AndroidSharedMemoryRegion {}
unsafe impl Sync for AndroidSharedMemoryRegion {}

impl AndroidSharedMemoryRegion {
    /// Returns the file descriptor for sharing with Java/Kotlin.
    #[cfg(target_os = "android")]
    pub fn fd(&self) -> RawFd {
        self.fd
    }

    #[cfg(not(target_os = "android"))]
    pub fn fd(&self) -> RawFd {
        -1
    }

    /// Returns the name of this region.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the total size including header.
    pub fn total_size(&self) -> usize {
        self.size
    }

    /// Returns a raw pointer to the entire buffer (header + data).
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr
    }
}

impl SharedMemoryRegion for AndroidSharedMemoryRegion {
    fn capacity(&self) -> usize {
        self.capacity
    }

    fn info(&self) -> Result<SharedStateInfo, SharedMemoryError> {
        let (version, length) = unsafe { read_header_ptr(self.ptr, self.capacity) }
            .ok_or(SharedMemoryError::InvalidHeader)?;

        Ok(SharedStateInfo {
            name: self.name.clone(),
            path: None,
            fd: Some(self.fd),
            version,
            length,
            capacity: self.capacity,
        })
    }

    fn write(&mut self, version: u64, data: &[u8]) -> Result<SharedStateInfo, SharedMemoryError> {
        if data.len() > self.capacity {
            return Err(SharedMemoryError::DataTooLarge {
                data_len: data.len(),
                capacity: self.capacity,
            });
        }

        let data_offset = HEADER_SIZE;

        // Write data
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), self.ptr.add(data_offset), data.len());
        }

        // Write header with memory barrier
        std::sync::atomic::fence(Ordering::Release);
        unsafe {
            write_header_ptr(self.ptr, version, data.len());
        }

        Ok(SharedStateInfo {
            name: self.name.clone(),
            path: None,
            fd: Some(self.fd),
            version,
            length: data.len(),
            capacity: self.capacity,
        })
    }

    fn read(&self) -> Result<Vec<u8>, SharedMemoryError> {
        let (_, length) = unsafe { read_header_ptr(self.ptr, self.capacity) }
            .ok_or(SharedMemoryError::InvalidHeader)?;

        let data_offset = HEADER_SIZE;
        let mut data = vec![0u8; length];

        unsafe {
            std::ptr::copy_nonoverlapping(self.ptr.add(data_offset), data.as_mut_ptr(), length);
        }

        Ok(data)
    }

    unsafe fn data_ptr(&self) -> *const u8 {
        unsafe { self.ptr.add(HEADER_SIZE) }
    }

    unsafe fn data_ptr_mut(&mut self) -> *mut u8 {
        unsafe { self.ptr.add(HEADER_SIZE) }
    }
}

#[cfg(target_os = "android")]
impl Drop for AndroidSharedMemoryRegion {
    fn drop(&mut self) {
        // Only clean up if this is the owning reference (fd >= 0)
        // Non-owning references have fd = -1 and should not clean up
        if self.fd >= 0 {
            unsafe {
                if !self.ptr.is_null() {
                    libc::munmap(self.ptr as *mut libc::c_void, self.size);
                }
                libc::close(self.fd);
            }
        }
    }
}

#[cfg(not(target_os = "android"))]
impl Drop for AndroidSharedMemoryRegion {
    fn drop(&mut self) {
        // No-op on non-Android platforms
    }
}

/// Factory for creating Android memio regions.
#[derive(Debug, Clone, Default)]
pub struct AndroidSharedMemoryFactory;

impl AndroidSharedMemoryFactory {
    /// Creates a new factory.
    pub fn new() -> Self {
        Self
    }
}

impl SharedMemoryFactory for AndroidSharedMemoryFactory {
    type Region = AndroidSharedMemoryRegion;

    #[cfg(target_os = "android")]
    fn create(&self, name: &str, capacity: usize) -> Result<Self::Region, SharedMemoryError> {
        use ndk::shared_memory::SharedMemory;
        use std::ffi::CString;
        use std::os::unix::io::IntoRawFd;
        use std::ptr;

        if capacity == 0 {
            return Err(SharedMemoryError::InvalidCapacity);
        }

        let total_size = HEADER_SIZE + capacity;

        // Create memio region with unique name
        let region_name = format!(
            "memio_{}_{}_{}",
            name,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let region_name_cstr = CString::new(region_name)
            .map_err(|_| SharedMemoryError::CreateFailed("Invalid name".into()))?;

        let shared_mem =
            SharedMemory::create(Some(&region_name_cstr), total_size).map_err(|e| {
                SharedMemoryError::CreateFailed(format!("ASharedMemory create failed: {:?}", e))
            })?;

        let fd = shared_mem.into_raw_fd();

        // Memory map the region
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                total_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };

        if ptr == libc::MAP_FAILED {
            unsafe { libc::close(fd) };
            return Err(SharedMemoryError::MmapFailed);
        }

        let ptr = ptr as *mut u8;

        // Initialize header
        unsafe {
            write_header_ptr(ptr, 0, 0);
        }

        // Store in registry - this is the ONLY copy that owns the resources
        let mut registry = REGISTRY.lock().unwrap();
        registry.insert(
            name.to_string(),
            AndroidSharedMemoryRegion {
                name: name.to_string(),
                fd,
                ptr,
                size: total_size,
                capacity,
            },
        );

        // Return a non-owning reference (fd = -1 means don't close on drop)
        Ok(AndroidSharedMemoryRegion {
            name: name.to_string(),
            fd: -1, // Non-owning reference
            ptr,
            size: total_size,
            capacity,
        })
    }

    #[cfg(not(target_os = "android"))]
    fn create(&self, _name: &str, _capacity: usize) -> Result<Self::Region, SharedMemoryError> {
        Err(SharedMemoryError::PlatformNotSupported)
    }

    fn open(&self, name: &str) -> Result<Self::Region, SharedMemoryError> {
        // On Android, we use the registry to find existing regions
        let registry = REGISTRY.lock().unwrap();
        let region = registry
            .get(name)
            .ok_or_else(|| SharedMemoryError::NotFound(name.to_string()))?;

        // Return a reference-like copy (doesn't own the fd)
        Ok(AndroidSharedMemoryRegion {
            name: region.name.clone(),
            #[cfg(target_os = "android")]
            fd: -1, // Don't own the fd
            ptr: region.ptr,
            size: region.size,
            capacity: region.capacity,
        })
    }

    fn list(&self) -> Vec<String> {
        let registry = REGISTRY.lock().unwrap();
        registry.keys().cloned().collect()
    }

    fn exists(&self, name: &str) -> bool {
        let registry = REGISTRY.lock().unwrap();
        registry.contains_key(name)
    }

    fn remove(&self, name: &str) -> Result<(), SharedMemoryError> {
        let mut registry = REGISTRY.lock().unwrap();
        registry
            .remove(name)
            .map(|_| ())
            .ok_or_else(|| SharedMemoryError::NotFound(name.to_string()))
    }
}

/// JNI-compatible API
/// Creates a new memio region and returns its file descriptor.
///
/// This is the primary entry point for JNI code.
pub fn create_shared_region(name: &str, capacity: usize) -> Result<RawFd, SharedMemoryError> {
    let factory = AndroidSharedMemoryFactory::new();
    let _region = factory.create(name, capacity)?;
    // Get the fd from the registry (the owning copy)
    get_shared_fd(name)
}

/// Writes data to a named memio region.
pub fn write_to_shared(name: &str, version: u64, data: &[u8]) -> Result<(), SharedMemoryError> {
    let mut registry = REGISTRY.lock().unwrap();
    let region = registry
        .get_mut(name)
        .ok_or_else(|| SharedMemoryError::NotFound(name.to_string()))?;
    region.write(version, data)?;
    Ok(())
}

/// Reads data from a named memio region.
pub fn read_from_shared(name: &str) -> Result<(u64, Vec<u8>), SharedMemoryError> {
    let registry = REGISTRY.lock().unwrap();
    let region = registry
        .get(name)
        .ok_or_else(|| SharedMemoryError::NotFound(name.to_string()))?;

    let info = region.info()?;
    let data = region.read()?;

    Ok((info.version, data))
}

/// Gets the file descriptor for a named memio region.
pub fn get_shared_fd(name: &str) -> Result<RawFd, SharedMemoryError> {
    let registry = REGISTRY.lock().unwrap();
    let region = registry
        .get(name)
        .ok_or_else(|| SharedMemoryError::NotFound(name.to_string()))?;
    Ok(region.fd())
}

/// Gets a raw pointer to the memio buffer.
pub fn get_shared_ptr(name: &str) -> Result<(*const u8, usize), SharedMemoryError> {
    let registry = REGISTRY.lock().unwrap();
    let region = registry
        .get(name)
        .ok_or_else(|| SharedMemoryError::NotFound(name.to_string()))?;
    Ok((region.as_ptr(), region.total_size()))
}

/// Lists all registered memio regions.
pub fn list_shared_regions() -> Vec<String> {
    let registry = REGISTRY.lock().unwrap();
    registry.keys().cloned().collect()
}

/// Checks if a memio region exists.
pub fn has_shared_region(name: &str) -> bool {
    let registry = REGISTRY.lock().unwrap();
    registry.contains_key(name)
}
