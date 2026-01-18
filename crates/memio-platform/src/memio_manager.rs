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

#[cfg(any(target_os = "android", target_os = "windows"))]
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

#[cfg(target_os = "windows")]
use crate::windows;

/// Unified manager for cross-platform memio region.
///
/// `MemioManager` provides a single, consistent API that works on supported platforms.
/// It handles platform-specific initialization and operations internally.
///
/// # Platform Behavior
///
/// - **Linux**: Uses `/dev/shm` POSIX memio region with memory-mapped files
/// - **Android**: Uses `ASharedMemory` (API 26+) with a global registry
///
/// # Thread Safety
///
/// `MemioManager` is thread-safe and can be shared across threads via `Arc`.
#[derive(Debug)]
pub struct MemioManager {
    #[cfg(target_os = "linux")]
    registry: Mutex<SharedRegistry<LinuxSharedMemoryFactory>>,

    #[cfg(target_os = "android")]
    buffers: Mutex<HashMap<String, BufferInfo>>,

    #[cfg(target_os = "windows")]
    buffers: Mutex<HashMap<String, BufferInfo>>,

    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows")))]
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
    ///
    /// # Linux
    /// Creates a new `SharedRegistry` with default paths in `/dev/shm/`.
    ///
    /// # Android
    /// Initializes an internal tracker (actual regions are created lazily).
    ///
    /// # Example
    /// ```ignore
    /// let manager = MemioManager::new()?;
    /// ```
    #[cfg(target_os = "linux")]
    pub fn new() -> Result<Self, SharedMemoryError> {
        // Clean up any orphaned files from previous sessions
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

    #[cfg(target_os = "windows")]
    pub fn new() -> Result<Self, SharedMemoryError> {
        Ok(Self {
            buffers: Mutex::new(HashMap::new()),
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows")))]
    pub fn new() -> Result<Self, SharedMemoryError> {
        Err(SharedMemoryError::PlatformNotSupported)
    }

    /// Creates a new memio buffer with the given name and capacity.
    ///
    /// # Arguments
    /// * `name` - Unique identifier for this buffer
    /// * `capacity` - Maximum size in bytes for the data (excluding header)
    ///
    /// # Example
    /// ```ignore
    /// manager.create_buffer("state", 1024 * 1024)?; // 1MB buffer
    /// ```
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

    #[cfg(target_os = "windows")]
    pub fn create_buffer(&self, name: &str, capacity: usize) -> Result<(), SharedMemoryError> {
        windows::create_shared_region(name, capacity)
            .map_err(|e| SharedMemoryError::CreateFailed(e))?;
        let mut buffers = self.buffers.lock()?;
        buffers.insert(name.to_string(), BufferInfo { capacity });
        Ok(())
    }

    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows")))]
    pub fn create_buffer(&self, _name: &str, _capacity: usize) -> Result<(), SharedMemoryError> {
        Err(SharedMemoryError::PlatformNotSupported)
    }

    /// Writes data to a memio buffer with versioning.
    ///
    /// # Arguments
    /// * `name` - Name of the buffer (must exist)
    /// * `version` - Version number for this write
    /// * `data` - Data bytes to write
    ///
    /// # Returns
    /// `WriteResult` containing the version and bytes written.
    ///
    /// # Example
    /// ```ignore
    /// let result = manager.write("state", 1, &serialized_data)?;
    /// println!("Wrote {} bytes at version {}", result.length, result.version);
    /// ```
    #[cfg(target_os = "linux")]
    pub fn write(
        &self,
        name: &str,
        version: u64,
        data: &[u8],
    ) -> Result<WriteResult, SharedMemoryError> {
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
    pub fn write(
        &self,
        name: &str,
        version: u64,
        data: &[u8],
    ) -> Result<WriteResult, SharedMemoryError> {
        android::write_to_shared(name, version, data)?;

        Ok(WriteResult {
            version,
            length: data.len(),
        })
    }

    #[cfg(target_os = "windows")]
    pub fn write(
        &self,
        name: &str,
        version: u64,
        data: &[u8],
    ) -> Result<WriteResult, SharedMemoryError> {
        windows::write_to_shared(name, version, data).map_err(|e| SharedMemoryError::Io(e))?;

        Ok(WriteResult {
            version,
            length: data.len(),
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows")))]
    pub fn write(
        &self,
        _name: &str,
        _version: u64,
        _data: &[u8],
    ) -> Result<WriteResult, SharedMemoryError> {
        Err(SharedMemoryError::PlatformNotSupported)
    }

    /// Reads data from a memio buffer.
    ///
    /// # Arguments
    /// * `name` - Name of the buffer to read from
    ///
    /// # Returns
    /// `ReadResult` containing the data and current version.
    ///
    /// # Example
    /// ```ignore
    /// let result = manager.read("state")?;
    /// let state: MyState = rkyv::from_bytes(&result.data)?;
    /// ```
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

        Ok(ReadResult { data, version })
    }

    #[cfg(target_os = "windows")]
    pub fn read(&self, name: &str) -> Result<ReadResult, SharedMemoryError> {
        let (version, data) =
            windows::read_from_shared(name).map_err(|e| SharedMemoryError::Io(e))?;

        Ok(ReadResult { data, version })
    }

    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows")))]
    pub fn read(&self, _name: &str) -> Result<ReadResult, SharedMemoryError> {
        Err(SharedMemoryError::PlatformNotSupported)
    }

    /// Gets the current version of a buffer (without reading data).
    ///
    /// This is useful for efficient polling - check if version changed before
    /// doing a full read.
    ///
    /// # Arguments
    /// * `name` - Name of the buffer
    ///
    /// # Returns
    /// Current version number.
    ///
    /// # Example
    /// ```ignore
    /// let current = manager.version("state")?;
    /// if current != last_version {
    ///     // Data changed, do a full read
    ///     let data = manager.read("state")?;
    /// }
    /// ```
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
        // Android: Read the info to get version
        // Note: The MemioJsBridge has a direct path for this on the JS side
        let (version, _) = android::read_from_shared(name)?;
        Ok(version)
    }

    #[cfg(target_os = "windows")]
    pub fn version(&self, name: &str) -> Result<u64, SharedMemoryError> {
        windows::get_version(name).map_err(|e| SharedMemoryError::Io(e))
    }

    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows")))]
    pub fn version(&self, _name: &str) -> Result<u64, SharedMemoryError> {
        Err(SharedMemoryError::PlatformNotSupported)
    }

    /// Gets detailed information about a buffer.
    ///
    /// # Arguments
    /// * `name` - Name of the buffer
    ///
    /// # Returns
    /// `SharedStateInfo` with version, length, capacity, etc.
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

    #[cfg(target_os = "windows")]
    pub fn info(&self, name: &str) -> Result<SharedStateInfo, SharedMemoryError> {
        let buffers = self.buffers.lock()?;

        let buffer_info = buffers
            .get(name)
            .ok_or_else(|| SharedMemoryError::NotFound(name.to_string()))?;

        let (version, data) =
            windows::read_from_shared(name).map_err(|e| SharedMemoryError::Io(e))?;

        Ok(SharedStateInfo {
            name: name.to_string(),
            path: None,
            fd: None,
            version,
            length: data.len(),
            capacity: buffer_info.capacity,
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows")))]
    pub fn info(&self, _name: &str) -> Result<SharedStateInfo, SharedMemoryError> {
        Err(SharedMemoryError::PlatformNotSupported)
    }

    /// Blocks until a buffer version changes or timeout is reached.
    ///
    /// Returns `Ok(Some(ReadResult))` when data changes, `Ok(None)` on timeout.
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
    ///
    /// # Arguments
    /// * `name` - Name of the buffer to check
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

    #[cfg(target_os = "windows")]
    pub fn has_buffer(&self, name: &str) -> bool {
        windows::has_shared_region(name)
    }

    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows")))]
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

    #[cfg(target_os = "windows")]
    pub fn list_buffers(&self) -> Vec<String> {
        windows::list_shared_regions()
    }

    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows")))]
    pub fn list_buffers(&self) -> Vec<String> {
        vec![]
    }

    /// Gets the registry (manifest) file path.
    ///
    /// This is useful for external processes that need to discover memio buffers.
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
        None // Android doesn't use file-based registry
    }

    #[cfg(target_os = "windows")]
    pub fn get_registry_path(&self) -> Option<std::path::PathBuf> {
        None // Windows uses named file mappings, not file-based registry
    }

    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows")))]
    pub fn get_registry_path(&self) -> Option<std::path::PathBuf> {
        None
    }
}

/// Creates a thread-safe, shareable `MemioManager`.
///
/// Function for creating an `Arc<MemioManager>` that
/// can be cloned and shared across threads and Tauri state.
///
/// # Example
/// ```ignore
/// let manager = memio_manager()?;
///
/// // Clone to share with Tauri
/// tauri::Builder::default()
///     .manage(manager.clone())
///     // ...
/// ```
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

        manager
            .create_buffer("test", 1024)
            .expect("Failed to create buffer");

        let data = b"Hello, MemioTauri!";
        let result = manager.write("test", 1, data).expect("Failed to write");

        assert_eq!(result.version, 1);
        assert_eq!(result.length, data.len());

        let read_result = manager.read("test").expect("Failed to read");
        assert_eq!(read_result.data, data);
        assert_eq!(read_result.version, 1);
    }
}
