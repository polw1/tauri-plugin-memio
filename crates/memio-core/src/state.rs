//! Thread-safe state container with serialization.

use rkyv::{Archive, Serialize};
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::SharedMemoryRegion;
use crate::error::{MemioError, MemioResult};
use crate::schema::{MemioSchema, schema_json};

/// State container with optional memio region binding.
pub struct MemioState<T, R: SharedMemoryRegion = NoOpRegion> {
    inner: RwLock<T>,
    version: AtomicU64,
    cache: RwLock<Option<(u64, Vec<u8>)>>,
    shared_region: RwLock<Option<R>>,
}

/// Placeholder region when memio region is not used.
#[derive(Debug, Default)]
pub struct NoOpRegion;

impl SharedMemoryRegion for NoOpRegion {
    fn capacity(&self) -> usize {
        0
    }

    fn info(&self) -> Result<crate::SharedStateInfo, MemioError> {
        Ok(crate::SharedStateInfo::default())
    }

    fn write(&mut self, _version: u64, _data: &[u8]) -> Result<crate::SharedStateInfo, MemioError> {
        Ok(crate::SharedStateInfo::default())
    }

    fn read(&self) -> Result<Vec<u8>, MemioError> {
        Ok(Vec::new())
    }

    unsafe fn data_ptr(&self) -> *const u8 {
        std::ptr::null()
    }
    unsafe fn data_ptr_mut(&mut self) -> *mut u8 {
        std::ptr::null_mut()
    }
}

impl<T> MemioState<T, NoOpRegion>
where
    T: Archive
        + for<'a> Serialize<
            rkyv::api::high::HighSerializer<
                rkyv::util::AlignedVec,
                rkyv::ser::allocator::ArenaHandle<'a>,
                rkyv::rancor::Error,
            >,
        >,
{
    /// Creates state without memio region.
    pub fn new(value: T) -> Self {
        Self {
            inner: RwLock::new(value),
            version: AtomicU64::new(0),
            cache: RwLock::new(None),
            shared_region: RwLock::new(None),
        }
    }

    /// Converts to use a memio region.
    pub fn with_shared_memory<R: SharedMemoryRegion>(self, region: R) -> MemioState<T, R> {
        MemioState {
            inner: self.inner,
            version: self.version,
            cache: self.cache,
            shared_region: RwLock::new(Some(region)),
        }
    }
}

impl<T, R: SharedMemoryRegion> MemioState<T, R>
where
    T: Archive
        + for<'a> Serialize<
            rkyv::api::high::HighSerializer<
                rkyv::util::AlignedVec,
                rkyv::ser::allocator::ArenaHandle<'a>,
                rkyv::rancor::Error,
            >,
        >,
{
    /// Creates state with memio region.
    pub fn new_with_region(value: T, region: R) -> Self {
        Self {
            inner: RwLock::new(value),
            version: AtomicU64::new(0),
            cache: RwLock::new(None),
            shared_region: RwLock::new(Some(region)),
        }
    }

    /// Serializes state to bytes.
    pub fn to_bytes(&self) -> MemioResult<Vec<u8>> {
        let guard = self.inner.read()?;
        serialize_value(&*guard)
    }

    /// Serializes state and caches result. Returns (version, bytes).
    pub fn to_bytes_cached(&self) -> MemioResult<(u64, Vec<u8>)> {
        let current_version = self.version();

        if let Ok(cache_guard) = self.cache.read()
            && let Some((cached_version, cached_bytes)) = cache_guard.as_ref()
            && *cached_version == current_version
        {
            return Ok((current_version, cached_bytes.clone()));
        }

        let bytes = self.to_bytes()?;
        if let Ok(mut cache_guard) = self.cache.write() {
            *cache_guard = Some((current_version, bytes.clone()));
        }

        Ok((current_version, bytes))
    }

    /// Serializes into arena. Returns (pointer, length).
    pub fn serialize_into(&self, arena: &crate::arena::Arena) -> MemioResult<(*const u8, usize)> {
        let bytes = self.to_bytes()?;
        let len = bytes.len();

        let ptr = arena.alloc(len, 8).ok_or(MemioError::ArenaFull {
            requested: len,
            available: arena.capacity() - arena.used(),
        })?;

        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.as_ptr(), len);
        }

        Ok((ptr.as_ptr() as *const u8, len))
    }

    /// Reads state with closure.
    pub fn read<F, R2>(&self, f: F) -> MemioResult<R2>
    where
        F: FnOnce(&T) -> R2,
    {
        let guard = self.inner.read()?;
        Ok(f(&*guard))
    }

    /// Writes state with closure. Syncs to memio region if bound.
    pub fn write<F, R2>(&self, f: F) -> MemioResult<R2>
    where
        F: FnOnce(&mut T) -> R2,
    {
        let mut guard = self.inner.write()?;
        let result = f(&mut *guard);
        let version = self.version.fetch_add(1, Ordering::SeqCst) + 1;

        let shared_enabled = self.shared_region.read()?.is_some();

        if shared_enabled {
            let bytes = serialize_value(&*guard)?;
            if let Ok(mut cache_guard) = self.cache.write() {
                *cache_guard = Some((version, bytes.clone()));
            }
            let mut shared_guard = self.shared_region.write()?;
            if let Some(region) = shared_guard.as_mut() {
                region.write(version, &bytes)?;
            }
        } else if let Ok(mut cache_guard) = self.cache.write() {
            *cache_guard = None;
        }

        Ok(result)
    }

    /// Returns current version number.
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Relaxed)
    }

    /// Returns memio region info if bound.
    pub fn shared_info(&self) -> Option<crate::SharedStateInfo> {
        self.shared_region
            .read()
            .ok()
            .and_then(|guard| guard.as_ref().and_then(|r| r.info().ok()))
    }

    /// Returns JSON schema of fields.
    pub fn schema_json(&self) -> String
    where
        T: MemioSchema,
    {
        schema_json::<T>()
    }
}

impl<T, R> Default for MemioState<T, R>
where
    T: Default
        + Archive
        + for<'a> Serialize<
            rkyv::api::high::HighSerializer<
                rkyv::util::AlignedVec,
                rkyv::ser::allocator::ArenaHandle<'a>,
                rkyv::rancor::Error,
            >,
        >,
    R: SharedMemoryRegion + Default,
{
    fn default() -> Self {
        Self {
            inner: RwLock::new(T::default()),
            version: AtomicU64::new(0),
            cache: RwLock::new(None),
            shared_region: RwLock::new(None),
        }
    }
}

fn serialize_value<T>(value: &T) -> MemioResult<Vec<u8>>
where
    T: Archive
        + for<'a> Serialize<
            rkyv::api::high::HighSerializer<
                rkyv::util::AlignedVec,
                rkyv::ser::allocator::ArenaHandle<'a>,
                rkyv::rancor::Error,
            >,
        >,
{
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(value)
        .map_err(|e| MemioError::Serialization(e.to_string()))?;
    Ok(bytes.to_vec())
}
