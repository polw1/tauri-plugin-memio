//! API for memio region management.
//!
//! Provides a unified interface for creating and managing memio buffers.
//!
//! # Example
//!
//! ```ignore
//! use memio_platform::MemioManager;
//!
//! let manager = MemioManager::new()?;
//!
//! // Create a memio buffer
//! manager.create_buffer("state", 1024 * 1024)?;
//!
//! // Write data (with version tracking)
//! let version = manager.write("state", 1, &my_data)?;
//!
//! // Read data
//! let data = manager.read("state")?;
//!
//! // Get version (for efficient polling)
//! let current_version = manager.version("state")?;
//! ```

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use memio_core::{SharedMemoryError, SharedStateInfo};

use crate::linux::LinuxSharedMemoryFactory;
use crate::registry::SharedRegistry;

/// Unified manager for Linux memio region.
#[derive(Debug)]
pub struct MemioManager {
    registry: Mutex<SharedRegistry<LinuxSharedMemoryFactory>>,
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
    /// Creates a new MemioManager for Linux.
    pub fn new() -> Result<Self, SharedMemoryError> {
        crate::cleanup_orphaned_files();

        let registry = SharedRegistry::new_linux()
            .map_err(|e| SharedMemoryError::CreateFailed(e.to_string()))?;
        Ok(Self {
            registry: Mutex::new(registry),
        })
    }

    /// Creates a new memio buffer with the given name and capacity.
    pub fn create_buffer(&self, name: &str, capacity: usize) -> Result<(), SharedMemoryError> {
        let mut registry = self.registry.lock()?;
        registry.create_buffer(name.to_string(), capacity)?;
        Ok(())
    }

    /// Writes data to a memio buffer with versioning.
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

    /// Reads data from a memio buffer.
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

    /// Gets the current version of a buffer (without reading data).
    pub fn version(&self, name: &str) -> Result<u64, SharedMemoryError> {
        let registry = self.registry.lock()?;

        let region = registry
            .get(name)
            .ok_or_else(|| SharedMemoryError::NotFound(name.to_string()))?;

        let info = region.info()?;
        Ok(info.version)
    }

    /// Gets detailed information about a buffer.
    pub fn info(&self, name: &str) -> Result<SharedStateInfo, SharedMemoryError> {
        let registry = self.registry.lock()?;

        let region = registry
            .get(name)
            .ok_or_else(|| SharedMemoryError::NotFound(name.to_string()))?;

        region.info()
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
    pub fn has_buffer(&self, name: &str) -> bool {
        if let Ok(registry) = self.registry.lock() {
            registry.get(name).is_some()
        } else {
            false
        }
    }

    /// Lists all buffer names.
    pub fn list_buffers(&self) -> Vec<String> {
        if let Ok(registry) = self.registry.lock() {
            registry.list_names()
        } else {
            vec![]
        }
    }

    /// Gets the registry (manifest) file path.
    pub fn get_registry_path(&self) -> Option<std::path::PathBuf> {
        if let Ok(registry) = self.registry.lock() {
            Some(registry.path().to_path_buf())
        } else {
            None
        }
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
