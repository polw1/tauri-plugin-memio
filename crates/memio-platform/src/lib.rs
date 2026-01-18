//! Memio region implementations for supported platforms.
//!
//! Includes platform-specific modules for memio region operations.
//!
//! # Supported Platforms
//!
//! - **Linux**: Uses `/dev/shm` for POSIX memio region via memory-mapped files
//!
//! # Usage
//!
//! ```ignore
//! use memio_platform::{platform_factory, Platform};
//!
//! // Get the appropriate factory for the current platform
//! let factory = platform_factory();
//!
//! // Create a memio region
//! let mut region = factory.create("my_state", 1024 * 1024)?;
//!
//! // Write data
//! region.write(1, b"hello world")?;
//! ```

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "android")]
pub mod android;

#[cfg(target_os = "windows")]
pub mod windows;

// Platform-specific utilities (Linux only for now)
#[cfg(target_os = "linux")]
pub mod shared_file;
#[cfg(target_os = "linux")]
pub mod shared_ring;

// High-level helpers
pub mod memio_shared;
pub mod registry;

/// Platform identifier for runtime detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    Linux,
    Android,
    MacOS,
    Windows,
    Wasm,
    Unknown,
}

impl Platform {
    /// Returns the current platform at compile time.
    pub const fn current() -> Self {
        #[cfg(target_os = "linux")]
        {
            Platform::Linux
        }
        #[cfg(target_os = "android")]
        {
            Platform::Android
        }
        #[cfg(target_os = "macos")]
        {
            Platform::MacOS
        }
        #[cfg(target_os = "windows")]
        {
            Platform::Windows
        }
        #[cfg(all(
            target_arch = "wasm32",
            not(any(
                target_os = "linux",
                target_os = "android",
                target_os = "macos",
                target_os = "windows"
            ))
        ))]
        {
            Platform::Wasm
        }
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "windows",
            target_arch = "wasm32"
        )))]
        {
            Platform::Unknown
        }
    }

    /// Returns a human-readable name for the platform.
    pub const fn name(&self) -> &'static str {
        match self {
            Platform::Linux => "linux",
            Platform::Android => "android",
            Platform::MacOS => "macos",
            Platform::Windows => "windows",
            Platform::Wasm => "wasm",
            Platform::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Returns the appropriate memio region factory for the current platform.
///
/// This function provides runtime platform selection, returning a boxed factory
/// that implements the `SharedMemoryFactory` trait.
///
/// # Panics
///
/// Panics if called on an unsupported platform.
///
/// # Example
///
/// ```ignore
/// use memio_platform::platform_factory;
///
/// let factory = platform_factory();
/// let region = factory.create("state", 4096)?;
/// ```
#[cfg(target_os = "linux")]
pub fn platform_factory() -> Box<linux::LinuxSharedMemoryFactory> {
    Box::new(linux::LinuxSharedMemoryFactory::new())
}

#[cfg(target_os = "android")]
pub fn platform_factory() -> Box<android::AndroidSharedMemoryFactory> {
    Box::new(android::AndroidSharedMemoryFactory::new())
}

#[cfg(target_os = "windows")]
pub fn platform_factory() -> Box<windows::WindowsSharedMemoryFactory> {
    Box::new(windows::WindowsSharedMemoryFactory::new())
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows")))]
pub fn platform_factory() -> ! {
    panic!(
        "No shared memory implementation available for platform: {}",
        Platform::current()
    )
}

// Re-exports for convenience
#[cfg(target_os = "linux")]
pub use linux::{LinuxSharedMemoryFactory, LinuxSharedMemoryRegion, cleanup_orphaned_files};

#[cfg(target_os = "android")]
pub use android::{AndroidSharedMemoryFactory, AndroidSharedMemoryRegion};

#[cfg(target_os = "windows")]
pub use windows::{WindowsSharedMemoryFactory, WindowsSharedMemoryRegion};

// Android JNI-compatible functions
#[cfg(target_os = "android")]
pub use android::{
    create_shared_region, get_shared_fd, get_shared_ptr, has_shared_region, list_shared_regions,
    read_from_shared, write_to_shared,
};

// Windows helper functions (similar API to Android)
#[cfg(target_os = "windows")]
pub use windows::{
    create_shared_region, get_version, has_shared_region, list_shared_regions, read_from_shared,
    write_to_shared,
};

// Linux-specific utilities
#[cfg(target_os = "linux")]
pub use shared_file::SharedFileCache;
#[cfg(target_os = "linux")]
pub use shared_ring::SharedRingBuffer;

// High-level helpers
pub mod memio_manager;
pub use memio_manager::{MemioManager, ReadResult, WriteResult, memio_manager};
#[cfg(target_os = "linux")]
pub use memio_shared::LinuxMemioShared;
pub use memio_shared::MemioShared;
pub use registry::SharedRegistry;

// Re-export core contracts
pub use memio_core::{
    BoxedFactory, BoxedRegion, SharedMemoryError, SharedMemoryFactory, SharedMemoryRegion,
    SharedStateInfo,
};

// Re-export header constants
pub use memio_core::{SHARED_STATE_HEADER_SIZE, SHARED_STATE_MAGIC};
