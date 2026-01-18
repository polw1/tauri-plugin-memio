//! Helpers for memio region operations.
//!
//! Provides wrappers for managing memio buffers.

use std::path::Path;

use crate::registry::SharedRegistry;
use memio_core::{MemioResult, SharedMemoryFactory};

/// Wrapper for managing memio buffers by name.
///
/// `MemioShared` combines a `SharedRegistry` with helpers
/// for common operations like creating buffers and enabling shared state.
///
/// # Example
/// ```ignore
/// use memio_platform::MemioShared;
///
/// let mut shared = MemioShared::new_linux()?;
/// shared.create_buffer("my_buffer", 4096)?;
///
/// // Use get_buffer to access the region
/// if let Ok(buffer) = shared.get_buffer("my_buffer") {
///     buffer.write(1, b"hello world")?;
/// }
/// ```
pub struct MemioShared<F: SharedMemoryFactory> {
    registry: SharedRegistry<F>,
}

impl<F: SharedMemoryFactory> MemioShared<F> {
    /// Creates a new MemioShared with the given registry.
    pub fn new(registry: SharedRegistry<F>) -> Self {
        Self { registry }
    }

    /// Registers an existing shared file path under a name.
    pub fn register(&mut self, name: impl Into<String>, path: impl AsRef<Path>) -> MemioResult<()> {
        self.registry.register(name, path)
    }

    /// Creates a new memio buffer and registers it under the provided name.
    pub fn create_buffer(&mut self, name: impl Into<String>, capacity: usize) -> MemioResult<()> {
        self.registry
            .create_buffer(name, capacity)
            .map_err(|e| memio_core::MemioError::Internal(e.to_string()))
    }

    /// Gets a mutable reference to a buffer by name.
    pub fn get_buffer(&mut self, name: &str) -> MemioResult<&mut F::Region> {
        self.registry
            .get_mut(name)
            .ok_or_else(|| memio_core::MemioError::Internal(format!("Buffer not found: {}", name)))
    }

    /// Returns the registry manifest path.
    pub fn registry_path(&self) -> &Path {
        self.registry.path()
    }
}

#[cfg(target_os = "linux")]
impl MemioShared<crate::LinuxSharedMemoryFactory> {
    /// Creates a new MemioShared with Linux defaults.
    ///
    /// The manifest is stored in `/dev/shm/memio_shared_registry_<pid>.txt`.
    pub fn new_linux() -> MemioResult<Self> {
        let registry = SharedRegistry::new_linux()?;
        Ok(Self::new(registry))
    }
}

// Type alias for convenience on Linux
#[cfg(target_os = "linux")]
pub type LinuxMemioShared = MemioShared<crate::LinuxSharedMemoryFactory>;
