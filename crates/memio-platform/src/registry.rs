//! Memio region registry for managing buffers.
//!
//! Provides a registry for named memio regions.

use std::collections::HashMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use memio_core::{MemioResult, SharedMemoryError, SharedMemoryFactory, SharedMemoryRegion};

/// Entry in the registry containing both path and region.
struct RegistryEntry<R> {
    path: PathBuf,
    region: R,
}

/// A registry that maps names to memio regions.
///
/// This is useful for managing multiple memio buffers and
/// making them discoverable by other processes via a manifest file.
pub struct SharedRegistry<F: SharedMemoryFactory> {
    factory: F,
    manifest_path: PathBuf,
    entries: HashMap<String, RegistryEntry<F::Region>>,
}

impl<F: SharedMemoryFactory> Debug for SharedRegistry<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedRegistry")
            .field("manifest_path", &self.manifest_path)
            .field("entry_count", &self.entries.len())
            .finish()
    }
}

impl<F: SharedMemoryFactory> SharedRegistry<F> {
    /// Creates a new registry with the given factory.
    pub fn new(factory: F, manifest_path: PathBuf) -> MemioResult<Self> {
        let registry = Self {
            factory,
            manifest_path,
            entries: HashMap::new(),
        };
        registry.set_env()?;
        Ok(registry)
    }

    /// Registers an existing path under a name (without a region).
    pub fn register(
        &mut self,
        _name: impl Into<String>,
        _path: impl AsRef<Path>,
    ) -> MemioResult<()> {
        // Note: This method is kept for API compatibility but doesn't store regions.
        // Use create_buffer for full functionality.
        self.write_manifest()
    }

    /// Creates a new memio buffer and registers it under the provided name.
    pub fn create_buffer(
        &mut self,
        name: impl Into<String>,
        capacity: usize,
    ) -> Result<(), SharedMemoryError> {
        let name = name.into();
        let region = self.factory.create(&name, capacity)?;
        let path = if let Ok(info) = region.info() {
            info.path.unwrap_or_default()
        } else {
            PathBuf::new()
        };

        self.entries.insert(name, RegistryEntry { path, region });
        let _ = self.write_manifest();
        Ok(())
    }

    /// Gets a reference to a region by name.
    pub fn get(&self, name: &str) -> Option<&F::Region> {
        self.entries.get(name).map(|e| &e.region)
    }

    /// Gets a mutable reference to a region by name.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut F::Region> {
        self.entries.get_mut(name).map(|e| &mut e.region)
    }

    /// Lists all registered buffer names.
    pub fn list_names(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    /// Returns the manifest path.
    pub fn path(&self) -> &Path {
        &self.manifest_path
    }

    /// Returns a reference to the underlying factory.
    pub fn factory(&self) -> &F {
        &self.factory
    }

    fn set_env(&self) -> MemioResult<()> {
        // SAFETY: Setting environment variable
        unsafe {
            std::env::set_var("MEMIO_SHARED_REGISTRY", &self.manifest_path);
        }
        Ok(())
    }

    fn write_manifest(&self) -> MemioResult<()> {
        let mut out = String::new();
        for (name, entry) in &self.entries {
            out.push_str(name);
            out.push('=');
            out.push_str(&entry.path.to_string_lossy());
            out.push('\n');
        }
        std::fs::write(&self.manifest_path, out)?;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl SharedRegistry<crate::LinuxSharedMemoryFactory> {
    /// Creates a new Linux registry with default settings.
    ///
    /// The manifest is stored in `/dev/shm/memio_shared_registry_<pid>.txt`.
    pub fn new_linux() -> MemioResult<Self> {
        let mut manifest_path = PathBuf::from("/dev/shm");
        manifest_path.push(format!("memio_shared_registry_{}.txt", std::process::id()));
        Self::new(crate::LinuxSharedMemoryFactory::new(), manifest_path)
    }
}

impl<F: SharedMemoryFactory> Drop for SharedRegistry<F> {
    fn drop(&mut self) {
        // Clean up manifest file when registry is dropped
        // Note: Individual regions will clean themselves up via their own Drop impl
        if self.manifest_path.exists()
            && let Err(e) = std::fs::remove_file(&self.manifest_path)
        {
            eprintln!(
                "Warning: Failed to remove manifest file {:?}: {}",
                self.manifest_path, e
            );
        }
    }
}
