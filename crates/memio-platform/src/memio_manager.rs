//! API for memio region management.
//!
//! Provides a unified interface for creating and managing memio buffers.

#[cfg(target_os = "android")]
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use memio_core::SharedMemoryRegion;
use memio_core::{SharedMemoryError, SharedStateInfo};

#[cfg(target_os = "linux")]
use crate::linux::LinuxSharedMemoryFactory;
#[cfg(target_os = "linux")]
use crate::registry::SharedRegistry;

#[cfg(target_os = "android")]
use crate::android;

/// Unified manager for cross-platform memio region.
#[derive(Debug)]
pub struct MemioManager {
    #[cfg(target_os = "linux")]
    registry: Mutex<SharedRegistry<LinuxSharedMemoryFactory>>,

    #[cfg(target_os = "android")]
    buffers: Mutex<HashMap<String, BufferInfo>>,

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    _phantom: std::marker::PhantomData<()>,
}

/// Information about a created buffer.
#[derive(Debug, Clone)]
#[cfg_attr(target_os = "linux", allow(dead_code))]
struct BufferInfo {
    capacity: usize,
}

/// Result of a write operation.
#[derive(Debug, Clone)]
pub struct WriteResult {
    /// The version number written
    pub version: u64,
    /// The number of bytes written
    pub length: usize,
}

/// Result of a read operation.
#[derive(Debug, Clone)]
pub struct ReadResult {
    /// The data read from memio region
    pub data: Vec<u8>,
    /// The version of the data
    pub version: u64,
}

impl MemioManager {
    /// Creates a new MemioManager for the current platform.
    #[cfg(target_os = "linux")]
    pub fn new() -> Result<Self, SharedMemoryError> {
        crate::cleanup_orphaned_files();

        let registry = SharedRegistry::new_linux()
            .map_err(|e| SharedMemoryError::CreateFailed(e.to_string()))?;
        Ok(Self {
            registry: Mutex::new(registry),
        })
    }

    #[cfg(target_os = "android")]
    pub fn new() -> Result<Self, SharedMemoryError> {
        Ok(Self {
            buffers: Mutex::new(HashMap::new()),
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub fn new() -> Result<Self, SharedMemoryError> {
        Err(SharedMemoryError::PlatformNotSupported)
    }

    /// Creates a new memio buffer with the given name and capacity.
    #[cfg(target_os = "linux")]
    pub fn create_buffer(&self, name: &str, capacity: usize) -> Result<(), SharedMemoryError> {
        let mut registry = self.registry.lock()?;
        registry.create_buffer(name.to_string(), capacity)?;
        Ok(())
    }

    #[cfg(target_os = "android")]
    pub fn create_buffer(&self, name: &str, capacity: usize) -> Result<(), SharedMemoryError> {
        android::create_shared_region(name, capacity)?;
        let mut buffers = self.buffers.lock()?;
        buffers.insert(name.to_string(), BufferInfo { capacity });
        Ok(())
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub fn create_buffer(&self, _name: &str, _capacity: usize) -> Result<(), SharedMemoryError> {
        Err(SharedMemoryError::PlatformNotSupported)
    }

    /// Writes data to a memio buffer with versioning.
    #[cfg(target_os = "linux")]
    pub fn write(&self, name: &str, version: u64, data: &[u8]) -> Result<WriteResult, SharedMemoryError> {
        let mut registry = self.registry.lock()?;

        let region = registry
            .get_mut(name)
            .ok_or_else(|| SharedMemoryError::NotFound(name.to_string()))?;

        let info = region.write(version, data)?;

        Ok(WriteResult {
            version: info.version,
            length: info.length,
        })
    }

    #[cfg(target_os = "android")]
    pub fn write(&self, name: &str, version: u64, data: &[u8]) -> Result<WriteResult, SharedMemoryError> {
        android::write_to_shared(name, version, data)?;

        Ok(WriteResult {
            version,
            length: data.len(),
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub fn write(&self, _name: &str, _version: u64, _data: &[u8]) -> Result<WriteResult, SharedMemoryError> {
        Err(SharedMemoryError::PlatformNotSupported)
    }

    /// Reads data from a memio buffer.
    #[cfg(target_os = "linux")]
    pub fn read(&self, name: &str) -> Result<ReadResult, SharedMemoryError> {
        let registry = self.registry.lock()?;

        let region = registry
            .get(name)
            .ok_or_else(|| SharedMemoryError::NotFound(name.to_string()))?;

        let info = region.info()?;
        let data = region.read()?;

        Ok(ReadResult {
            data,
            version: info.version,
        })
    }

    #[cfg(target_os = "android")]
    pub fn read(&self, name: &str) -> Result<ReadResult, SharedMemoryError> {
        let (version, data) = android::read_from_shared(name)?;

        Ok(ReadResult {
            data,
            version,
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub fn read(&self, _name: &str) -> Result<ReadResult, SharedMemoryError> {
        Err(SharedMemoryError::PlatformNotSupported)
    }

    /// Gets the current version of a buffer (without reading data).
    #[cfg(target_os = "linux")]
    pub fn version(&self, name: &str) -> Result<u64, SharedMemoryError> {
        let registry = self.registry.lock()?;

        let region = registry
            .get(name)
            .ok_or_else(|| SharedMemoryError::NotFound(name.to_string()))?;

        let info = region.info()?;
        Ok(info.version)
    }

    #[cfg(target_os = "android")]
    pub fn version(&self, name: &str) -> Result<u64, SharedMemoryError> {
        let (version, _) = android::read_from_shared(name)?;
        Ok(version)
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub fn version(&self, _name: &str) -> Result<u64, SharedMemoryError> {
        Err(SharedMemoryError::PlatformNotSupported)
    }

    /// Gets detailed information about a buffer.
    #[cfg(target_os = "linux")]
    pub fn info(&self, name: &str) -> Result<SharedStateInfo, SharedMemoryError> {
        let registry = self.registry.lock()?;

        let region = registry
            .get(name)
            .ok_or_else(|| SharedMemoryError::NotFound(name.to_string()))?;

        region.info()
    }

    #[cfg(target_os = "android")]
    pub fn info(&self, name: &str) -> Result<SharedStateInfo, SharedMemoryError> {
        let buffers = self.buffers.lock()?;

        let buffer_info = buffers
            .get(name)
            .ok_or_else(|| SharedMemoryError::NotFound(name.to_string()))?;

        let (version, data) = android::read_from_shared(name)?;
        let fd = android::get_shared_fd(name).ok();

        Ok(SharedStateInfo {
            name: name.to_string(),
            path: None,
            fd,
            version,
            length: data.len(),
            capacity: buffer_info.capacity,
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub fn info(&self, _name: &str) -> Result<SharedStateInfo, SharedMemoryError> {
        Err(SharedMemoryError::PlatformNotSupported)
    }

    /// Blocks until a buffer version changes or timeout is reached.
    pub fn wait_for_change(
        &self,
        name: &str,
        last_version: u64,
        timeout: Duration,
        poll_interval: Duration,
    ) -> Result<Option<ReadResult>, SharedMemoryError> {
        let start = Instant::now();
        loop {
            let current = self.version(name)?;
            if current != last_version {
                return Ok(Some(self.read(name)?));
            }
            if start.elapsed() >= timeout {
                return Ok(None);
            }
            std::thread::sleep(poll_interval);
        }
    }

    /// Checks if a buffer exists.
    #[cfg(target_os = "linux")]
    pub fn has_buffer(&self, name: &str) -> bool {
        if let Ok(registry) = self.registry.lock() {
            registry.get(name).is_some()
        } else {
            false
        }
    }

    #[cfg(target_os = "android")]
    pub fn has_buffer(&self, name: &str) -> bool {
        android::has_shared_region(name)
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub fn has_buffer(&self, _name: &str) -> bool {
        false
    }

    /// Lists all buffer names.
    #[cfg(target_os = "linux")]
    pub fn list_buffers(&self) -> Vec<String> {
        if let Ok(registry) = self.registry.lock() {
            registry.list_names()
        } else {
            vec![]
        }
    }

    #[cfg(target_os = "android")]
    pub fn list_buffers(&self) -> Vec<String> {
        android::list_shared_regions()
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub fn list_buffers(&self) -> Vec<String> {
        vec![]
    }

    /// Gets the registry (manifest) file path.
    #[cfg(target_os = "linux")]
    pub fn get_registry_path(&self) -> Option<std::path::PathBuf> {
        if let Ok(registry) = self.registry.lock() {
            Some(registry.path().to_path_buf())
        } else {
            None
        }
    }

    #[cfg(target_os = "android")]
    pub fn get_registry_path(&self) -> Option<std::path::PathBuf> {
        None
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub fn get_registry_path(&self) -> Option<std::path::PathBuf> {
        None
    }
}

/// Creates a thread-safe, shareable `MemioManager`.
pub fn memio_manager() -> Result<Arc<MemioManager>, SharedMemoryError> {
    Ok(Arc::new(MemioManager::new()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn test_create_and_write() {
        let manager = MemioManager::new().expect("Failed to create manager");

        manager.create_buffer("test", 1024).expect("Failed to create buffer");

        let data = b"Hello, MemioTauri!";
        let result = manager.write("test", 1, data).expect("Failed to write");

        assert_eq!(result.version, 1);
        assert_eq!(result.length, data.len());

        let read_result = manager.read("test").expect("Failed to read");
        assert_eq!(read_result.data, data);
        assert_eq!(read_result.version, 1);
    }
}
