//! Types and traits for state management and memio region.

use std::fmt::Debug;
use std::path::PathBuf;

pub mod arena;
pub mod error;
pub mod schema;
pub mod shared_header;
pub mod shared_state;
mod shared_state_spec;
pub mod state;

pub use arena::Arena;
pub use error::{MemioError, MemioResult};

/// Alias for MemioError.
pub type SharedMemoryError = MemioError;

/// Metadata for a memio region.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SharedStateInfo {
    pub name: String,
    pub path: Option<PathBuf>,
    pub fd: Option<i32>,
    pub version: u64,
    pub length: usize,
    pub capacity: usize,
}

/// Interface for memio regions.
pub trait SharedMemoryRegion: Send + Sync + Debug {
    /// Returns data capacity in bytes.
    fn capacity(&self) -> usize;

    /// Returns region metadata.
    fn info(&self) -> Result<SharedStateInfo, MemioError>;

    /// Writes data with version number.
    fn write(&mut self, version: u64, data: &[u8]) -> Result<SharedStateInfo, MemioError>;

    /// Reads data bytes.
    fn read(&self) -> Result<Vec<u8>, MemioError>;

    /// Returns pointer to data area.
    /// # Safety
    /// Implementations must return a valid pointer for the region's lifetime.
    unsafe fn data_ptr(&self) -> *const u8;

    /// Returns mutable pointer to data area.
    /// # Safety
    /// Implementations must return a valid mutable pointer for the region's lifetime.
    unsafe fn data_ptr_mut(&mut self) -> *mut u8;
}

/// Interface for creating memio regions.
pub trait SharedMemoryFactory: Send + Sync {
    type Region: SharedMemoryRegion;

    /// Creates a region with name and capacity.
    fn create(&self, name: &str, capacity: usize) -> Result<Self::Region, MemioError>;

    /// Opens an existing region by name.
    fn open(&self, name: &str) -> Result<Self::Region, MemioError>;

    /// Lists region names.
    fn list(&self) -> Vec<String>;

    /// Checks if region exists.
    fn exists(&self, name: &str) -> bool;

    /// Removes a region.
    fn remove(&self, name: &str) -> Result<(), MemioError>;
}

pub type BoxedRegion = Box<dyn SharedMemoryRegion>;
pub type BoxedFactory = Box<dyn SharedMemoryFactory<Region = BoxedRegion>>;

pub use schema::{MemioField, MemioFieldType, MemioScalarType, MemioSchema, schema_json};
pub use shared_state::{SHARED_STATE_HEADER_SIZE, SHARED_STATE_MAGIC};
pub use state::{MemioState, NoOpRegion};

pub use shared_header::{
    SHARED_STATE_ENDIANNESS, SHARED_STATE_LENGTH_OFFSET, SHARED_STATE_MAGIC_OFFSET,
    SHARED_STATE_VERSION_OFFSET, read_header, read_header_ptr, read_length, read_u64_le,
    read_u64_ptr, read_version, validate_magic, validate_magic_result, write_header,
    write_header_ptr, write_header_unchecked, write_u64_le, write_u64_ptr,
};

pub use memio_macros::MemioModel;
pub use rkyv;
