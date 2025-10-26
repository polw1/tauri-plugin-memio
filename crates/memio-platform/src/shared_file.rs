//! Shared file cache implementation.
//!
//! Provides a mechanism for caching files in memory.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
#[cfg(target_os = "linux")]
use std::sync::atomic::Ordering;

use memio_core::MemioResult;

static SHARED_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A file cache that copies files to `/dev/shm`.
///
/// This is useful for caching files that need to be accessed frequently
/// by other processes, avoiding disk I/O.
pub struct SharedFileCache {
    dest_path: PathBuf,
    last_size: u64,
    last_mtime: u128,
}

impl SharedFileCache {
    /// Creates a new shared file cache.
    ///
    /// On Linux, this creates a destination path in `/dev/shm`.
    pub fn new() -> MemioResult<Self> {
        #[cfg(not(target_os = "linux"))]
        {
            return Err(MemioError::Internal("SharedFileCache is Linux-only.".to_string()));
        }

        #[cfg(target_os = "linux")]
        {
            let mut path = PathBuf::from("/dev/shm");
            let id = SHARED_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
            path.push(format!("memio_shared_{}_{}.bin", std::process::id(), id));
            Ok(Self {
                dest_path: path,
                last_size: 0,
                last_mtime: 0,
            })
        }
    }

    /// Copies the source file to `/dev/shm` if it has changed.
    ///
    /// Returns the path to the cached file.
    pub fn copy_if_changed(&mut self, source: &Path) -> MemioResult<PathBuf> {
        let metadata = source.metadata()?;
        let size = metadata.len();
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis())
            .unwrap_or(0);

        if size != self.last_size || mtime != self.last_mtime {
            std::fs::copy(source, &self.dest_path)?;
            self.last_size = size;
            self.last_mtime = mtime;
        }

        Ok(self.dest_path.clone())
    }

    /// Returns the destination path in `/dev/shm`.
    pub fn dest_path(&self) -> &Path {
        &self.dest_path
    }
}
