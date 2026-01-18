//! Linux memio region implementation.
//!
//! Provides memio region functionality using memory-mapped files.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use memmap2::MmapMut;
use once_cell::sync::Lazy;

use memio_core::{
    SHARED_STATE_HEADER_SIZE, SharedMemoryError, SharedMemoryFactory, SharedMemoryRegion,
    SharedStateInfo, read_header, validate_magic, write_header_unchecked,
};

const HEADER_SIZE: usize = SHARED_STATE_HEADER_SIZE;

/// Counter for generating unique file names
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Base directory for memio files
const SHM_BASE_PATH: &str = "/dev/shm";

/// Prefix for MemioTauri memio files
const FILE_PREFIX: &str = "memio_";

/// Registry tracking created regions for listing
static REGISTRY: Lazy<Mutex<HashMap<String, PathBuf>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Cleans up orphaned memio files from previous sessions.
///
/// This function removes any `memio_*` files in `/dev/shm` that belong to
/// processes that are no longer running. It's safe to call at startup.
///
/// # Example
/// ```ignore
/// memio_platform::linux::cleanup_orphaned_files();
/// ```
pub fn cleanup_orphaned_files() {
    let shm_path = Path::new(SHM_BASE_PATH);

    if let Ok(entries) = fs::read_dir(shm_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                // Only process memio_* files
                if !filename.starts_with(FILE_PREFIX) {
                    continue;
                }

                // Extract PID from filename (format: memio_<name>_<pid>_<nonce>_<seq>.bin)
                // or memio_shared_registry_<pid>.txt
                if let Some(pid) = extract_pid_from_filename(filename) {
                    // Check if process is still running
                    if !is_process_running(pid) {
                        if let Err(e) = fs::remove_file(&path) {
                            eprintln!("Warning: Failed to remove orphaned file {:?}: {}", path, e);
                        } else {
                            eprintln!("Cleaned up orphaned memio file: {:?}", path);
                        }
                    }
                }
            }
        }
    }
}

/// Extracts PID from a memio filename.
fn extract_pid_from_filename(filename: &str) -> Option<u32> {
    // Handle registry files: memio_shared_registry_<pid>.txt
    if filename.starts_with("memio_shared_registry_") && filename.ends_with(".txt") {
        let middle = filename
            .strip_prefix("memio_shared_registry_")?
            .strip_suffix(".txt")?;
        return middle.parse().ok();
    }

    // Handle data files: memio_<name>_<pid>_<nonce>_<seq>.bin
    if filename.ends_with(".bin") {
        let parts: Vec<&str> = filename.split('_').collect();
        // Need at least: memio, name, pid, nonce, seq.bin
        if parts.len() >= 5 {
            // PID is third from the end (before nonce and seq.bin)
            let pid_idx = parts.len() - 3;
            return parts[pid_idx].parse().ok();
        }
    }

    None
}

/// Checks if a process with the given PID is still running.
fn is_process_running(pid: u32) -> bool {
    // On Linux, check if /proc/<pid> exists
    Path::new(&format!("/proc/{}", pid)).exists()
}

/// Linux memio region using memory-mapped files.
#[derive(Debug)]
pub struct LinuxSharedMemoryRegion {
    name: String,
    path: PathBuf,
    mmap: MmapMut,
    capacity: usize,
}

impl LinuxSharedMemoryRegion {
    /// Returns the file path of this memio region.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the name of this region.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for LinuxSharedMemoryRegion {
    fn drop(&mut self) {
        // Clean up the memio file when the region is dropped
        if self.path.exists()
            && let Err(e) = std::fs::remove_file(&self.path)
        {
            eprintln!(
                "Warning: Failed to remove memio file {:?}: {}",
                self.path, e
            );
        }
        // Remove from registry
        if let Ok(mut registry) = REGISTRY.lock() {
            registry.remove(&self.name);
        }
    }
}

impl SharedMemoryRegion for LinuxSharedMemoryRegion {
    fn capacity(&self) -> usize {
        self.capacity
    }

    fn info(&self) -> Result<SharedStateInfo, SharedMemoryError> {
        let (version, length) =
            read_header(&self.mmap, self.capacity).ok_or(SharedMemoryError::InvalidHeader)?;

        Ok(SharedStateInfo {
            name: self.name.clone(),
            path: Some(self.path.clone()),
            fd: None,
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

        // Write data after header
        let data_offset = HEADER_SIZE;
        self.mmap[data_offset..data_offset + data.len()].copy_from_slice(data);

        // Write header (includes magic, version, length)
        write_header_unchecked(&mut self.mmap, version, data.len());

        // Ensure changes are visible
        self.mmap
            .flush()
            .map_err(|e| SharedMemoryError::Io(e.to_string()))?;

        Ok(SharedStateInfo {
            name: self.name.clone(),
            path: Some(self.path.clone()),
            fd: None,
            version,
            length: data.len(),
            capacity: self.capacity,
        })
    }

    fn read(&self) -> Result<Vec<u8>, SharedMemoryError> {
        let (_, length) =
            read_header(&self.mmap, self.capacity).ok_or(SharedMemoryError::InvalidHeader)?;

        let data_offset = HEADER_SIZE;
        let mut data = vec![0u8; length];
        data.copy_from_slice(&self.mmap[data_offset..data_offset + length]);

        Ok(data)
    }

    unsafe fn data_ptr(&self) -> *const u8 {
        // SAFETY: mmap is valid and HEADER_SIZE is within bounds
        unsafe { self.mmap.as_ptr().add(HEADER_SIZE) }
    }

    unsafe fn data_ptr_mut(&mut self) -> *mut u8 {
        // SAFETY: mmap is valid and HEADER_SIZE is within bounds
        unsafe { self.mmap.as_mut_ptr().add(HEADER_SIZE) }
    }
}

/// Factory for creating Linux memio regions.
#[derive(Debug, Clone)]
pub struct LinuxSharedMemoryFactory {
    base_path: PathBuf,
}

impl LinuxSharedMemoryFactory {
    /// Creates a new factory with the default base path (`/dev/shm`).
    pub fn new() -> Self {
        Self {
            base_path: PathBuf::from(SHM_BASE_PATH),
        }
    }

    /// Creates a new factory with a custom base path.
    ///
    /// Useful for testing or when `/dev/shm` is not available.
    pub fn with_base_path(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }

    /// Generates a unique file path for a new region.
    fn generate_path(&self, name: &str) -> PathBuf {
        let pid = std::process::id();
        let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
        let filename = format!("{}{}_{}_{}_{}.bin", FILE_PREFIX, name, pid, nonce, 0);
        self.base_path.join(filename)
    }

    /// Opens or creates a memio file.
    fn open_or_create(
        &self,
        name: &str,
        path: PathBuf,
        capacity: usize,
        create: bool,
    ) -> Result<LinuxSharedMemoryRegion, SharedMemoryError> {
        let file_len = HEADER_SIZE + capacity;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .open(&path)
            .map_err(|e| {
                if create {
                    SharedMemoryError::CreateFailed(e.to_string())
                } else {
                    SharedMemoryError::OpenFailed(e.to_string())
                }
            })?;

        if create {
            file.set_len(file_len as u64)
                .map_err(|e| SharedMemoryError::CreateFailed(e.to_string()))?;
        }

        let mut mmap =
            unsafe { MmapMut::map_mut(&file).map_err(|_| SharedMemoryError::MmapFailed)? };

        if create {
            // Initialize header with version 0 and length 0
            write_header_unchecked(&mut mmap, 0, 0);
        } else {
            // Validate existing header
            if !validate_magic(&mmap) {
                return Err(SharedMemoryError::InvalidHeader);
            }
        }

        // Track in registry
        {
            let mut registry = REGISTRY.lock().unwrap();
            registry.insert(name.to_string(), path.clone());
        }

        Ok(LinuxSharedMemoryRegion {
            name: name.to_string(),
            path,
            mmap,
            capacity,
        })
    }
}

impl Default for LinuxSharedMemoryFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedMemoryFactory for LinuxSharedMemoryFactory {
    type Region = LinuxSharedMemoryRegion;

    fn create(&self, name: &str, capacity: usize) -> Result<Self::Region, SharedMemoryError> {
        if capacity == 0 {
            return Err(SharedMemoryError::InvalidCapacity);
        }

        let path = self.generate_path(name);
        self.open_or_create(name, path, capacity, true)
    }

    fn open(&self, name: &str) -> Result<Self::Region, SharedMemoryError> {
        // First check registry for path
        let path = {
            let registry = REGISTRY.lock().unwrap();
            registry.get(name).cloned()
        };

        let path = path.ok_or_else(|| SharedMemoryError::NotFound(name.to_string()))?;

        // Get file size to determine capacity
        let metadata =
            fs::metadata(&path).map_err(|e| SharedMemoryError::OpenFailed(e.to_string()))?;
        let file_len = metadata.len() as usize;

        if file_len < HEADER_SIZE {
            return Err(SharedMemoryError::InvalidHeader);
        }

        let capacity = file_len - HEADER_SIZE;
        self.open_or_create(name, path, capacity, false)
    }

    fn list(&self) -> Vec<String> {
        let registry = REGISTRY.lock().unwrap();
        registry.keys().cloned().collect()
    }

    fn exists(&self, name: &str) -> bool {
        let registry = REGISTRY.lock().unwrap();
        if let Some(path) = registry.get(name) {
            path.exists()
        } else {
            false
        }
    }

    fn remove(&self, name: &str) -> Result<(), SharedMemoryError> {
        let path = {
            let mut registry = REGISTRY.lock().unwrap();
            registry.remove(name)
        };

        if let Some(path) = path {
            fs::remove_file(&path).map_err(|e| SharedMemoryError::Io(e.to_string()))?;
            Ok(())
        } else {
            Err(SharedMemoryError::NotFound(name.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn test_factory() -> LinuxSharedMemoryFactory {
        // Use temp dir for tests
        let temp_dir = env::temp_dir().join("memio_test");
        fs::create_dir_all(&temp_dir).unwrap();
        LinuxSharedMemoryFactory::with_base_path(temp_dir)
    }

    #[test]
    fn test_create_and_write() {
        let factory = test_factory();
        let mut region = factory.create("test1", 1024).unwrap();

        let info = region.write(1, b"hello world").unwrap();
        assert_eq!(info.version, 1);
        assert_eq!(info.length, 11);
        assert_eq!(info.capacity, 1024);

        let data = region.read().unwrap();
        assert_eq!(data, b"hello world");

        factory.remove("test1").unwrap();
    }

    #[test]
    fn test_capacity_exceeded() {
        let factory = test_factory();
        let mut region = factory.create("test2", 10).unwrap();

        let result = region.write(1, b"this is too long");
        assert!(matches!(
            result,
            Err(SharedMemoryError::DataTooLarge { .. })
        ));

        factory.remove("test2").unwrap();
    }

    #[test]
    fn test_list_and_exists() {
        let factory = test_factory();
        let _ = factory.create("list_test", 100).unwrap();

        assert!(factory.exists("list_test"));
        assert!(!factory.exists("nonexistent"));

        let list = factory.list();
        assert!(list.contains(&"list_test".to_string()));

        factory.remove("list_test").unwrap();
    }
}
