//! # MemioTauri Framework
//!
//! Memio region(or close memio region approach) for Tauri applications.
//!
//! This is the **single entry point** for MemioTauri. Just add `memio` to your
//! dependencies and you have everything you need!
//!
//! # Quick Start
//!
//! ```toml
//! [dependencies]
//! memio = { path = "path/to/memio" }
//! tauri = "2"
//! ```
//!
//! ```rust,ignore
//! use memio::prelude::*;
//!
//! // Create a unified manager (works on Linux and Android)
//! let manager = MemioManager::new()?;
//! manager.create_buffer("state", 1024 * 1024)?;
//! manager.write("state", 1, &data)?;
//! ```
//!
//! # Module Organization
//!
//! - [`prelude`] - Import everything you need with `use memio::prelude::*`
//! - [`MemioManager`] - Cross-platform unified API
//! - [`MemioState`] - Type-safe state container with versioning
//! - [`plugin`] - Tauri plugin (use `memio::plugin::init()`)

// ============================================================================
// RE-EXPORTS: Everything the user needs from one place
// ============================================================================

// Core types
pub use memio_core::{
    Arena,
    MemioError,
    MemioField,
    MemioFieldType,
    MemioModel, // Derive macro
    MemioResult,
    MemioScalarType,
    MemioSchema,
    MemioState,
    NoOpRegion,
    SharedMemoryError,
    SharedMemoryFactory,
    SharedMemoryRegion,
    SharedStateInfo,
};

// Unified cross-platform API
pub use memio_platform::{
    memio_manager, platform_factory, MemioManager, Platform, ReadResult, WriteResult,
};

// Re-export rkyv for serialization
pub use rkyv;

// ============================================================================
// TAURI PLUGIN
// ============================================================================

/// Tauri plugin for MemioTauri.
///
/// # Usage
///
/// ```rust,ignore
/// use memio::plugin;
///
/// tauri::Builder::default()
///     .plugin(memio::plugin::init())
///     .setup(|app| {
///         #[cfg(target_os = "linux")]
///         memio::plugin::build_webview_windows(app)?;
///         Ok(())
///     })
/// ```
pub mod plugin {
    pub use tauri_plugin_memio::init;

    #[cfg(target_os = "linux")]
    pub use tauri_plugin_memio::build_webview_windows;
}

// ============================================================================
// PRELUDE: Import everything with `use memio::prelude::*`
// ============================================================================

/// Prelude module - import everything you need with one line.
///
/// ```rust,ignore
/// use memio::prelude::*;
/// ```
pub mod prelude {
    // Core types
    pub use crate::{
        MemioError, MemioManager, MemioResult, MemioState, ReadResult, SharedStateInfo, WriteResult,
    };

    // Derive macro
    pub use memio_core::MemioModel;

    // rkyv traits for serialization
    pub use rkyv::{Archive, Deserialize, Serialize};
}

// ============================================================================
// PLATFORM-SPECIFIC APIs (For advanced use cases only)
// ============================================================================

/// Platform-specific implementations for advanced use cases.
///
/// Most users should use [`MemioManager`] instead.
pub mod platform {
    #[cfg(target_os = "linux")]
    pub use memio_platform::{
        LinuxMemioShared, LinuxSharedMemoryFactory, LinuxSharedMemoryRegion, MemioShared,
        SharedFileCache, SharedRegistry, SharedRingBuffer,
    };

    #[cfg(target_os = "android")]
    pub use memio_platform::{
        get_shared_ptr, has_shared_region, list_shared_regions, write_to_shared,
        AndroidSharedMemoryFactory, AndroidSharedMemoryRegion, SHARED_STATE_HEADER_SIZE,
        SHARED_STATE_MAGIC,
    };
}
#[cfg(target_os = "android")]
mod android_jni;
