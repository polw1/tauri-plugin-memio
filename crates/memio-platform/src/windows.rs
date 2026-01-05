//! Windows memio region implementation.
//!
//! Provides memio region functionality using Windows File Mapping API.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use once_cell::sync::Lazy;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile,
    FILE_MAP_ALL_ACCESS, PAGE_READWRITE, MEMORY_MAPPED_VIEW_ADDRESS,
};

use memio_core::{
    read_header, write_header_unchecked, SharedMemoryError, SharedMemoryFactory,
    SharedMemoryRegion, SharedStateInfo, SHARED_STATE_HEADER_SIZE, MemioError,
};

const HEADER_SIZE: usize = SHARED_STATE_HEADER_SIZE;

/// Counter for generating unique mapping names
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Prefix for MemioTauri named file mappings
const MAPPING_PREFIX: &str = "Local\\MemioTauri_";

/// Registry tracking created regions: name -> (mapping_name, capacity)
static REGISTRY: Lazy<Mutex<HashMap<String, (String, usize)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Storage for active regions to keep them alive
static ACTIVE_REGIONS: Lazy<Mutex<HashMap<String, RegionHandle>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Internal handle storage to keep Windows handles alive
struct RegionHandle {
    handle: HANDLE,
    ptr: *mut u8,
    total_size: usize,
    capacity: usize,
}

// SAFETY: Handle and ptr are only used in synchronized operations
unsafe impl Send for RegionHandle {}
unsafe impl Sync for RegionHandle {}

/// Windows memio region using File Mapping.
#[derive(Debug)]
pub struct WindowsSharedMemoryRegion {
    /// User-facing name
    name: String,
    /// Full mapping name
    mapping_name: String,
    /// Windows File Mapping handle
    handle: HANDLE,
    /// Pointer to mapped memory
    ptr: *mut u8,
    /// Total size including header
    total_size: usize,
    /// Data capacity (excluding header)
    capacity: usize,
    /// Whether this region owns the handle (created vs opened)
    owns_handle: bool,
}

// SAFETY: The raw pointer and handle are only used within synchronized operations
unsafe impl Send for WindowsSharedMemoryRegion {}
unsafe impl Sync for WindowsSharedMemoryRegion {}

impl WindowsSharedMemoryRegion {
    /// Creates a new named file mapping.
    pub fn create(name: &str, capacity: usize) -> Result<Self, MemioError> {
        let total_size = HEADER_SIZE + capacity;

        // Generate unique mapping name
        let pid = std::process::id();
        let counter = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mapping_name = format!("{}{}_{}_{}", MAPPING_PREFIX, name, pid, counter);

        // Convert to wide string (UTF-16)
        let wide_name: Vec<u16> = OsStr::new(&mapping_name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // Create named file mapping
        let handle = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                ((total_size as u64) >> 32) as u32,
                total_size as u32,
                PCWSTR::from_raw(wide_name.as_ptr()),
            )
        }
        .map_err(|e| MemioError::CreateFailed(format!("CreateFileMappingW failed: {:?}", e)))?;

        // Map the view
        let ptr = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, total_size) };

        if ptr.Value.is_null() {
            unsafe { let _ = CloseHandle(handle); }
            return Err(MemioError::MmapFailed);
        }

        // Initialize header (version=0, length=0)
        let header_slice = unsafe { std::slice::from_raw_parts_mut(ptr.Value as *mut u8, HEADER_SIZE) };
        write_header_unchecked(header_slice, 0, 0);

        // Register with capacity info
        {
            let mut registry = REGISTRY.lock().unwrap();
            registry.insert(name.to_string(), (mapping_name.clone(), capacity));
        }

        tracing::debug!(
            "Created Windows shared memory: {} ({} bytes)",
            mapping_name, total_size
        );

        Ok(Self {
            name: name.to_string(),
            mapping_name,
            handle,
            ptr: ptr.Value as *mut u8,
            total_size,
            capacity,
            owns_handle: true,
        })
    }

    /// Opens an existing named file mapping.
    pub fn open(name: &str) -> Result<Self, MemioError> {
        // Look up full mapping name and capacity from registry
        let (mapping_name, capacity) = {
            let registry = REGISTRY.lock().unwrap();
            registry.get(name).cloned()
        }.ok_or_else(|| MemioError::NotFound(name.to_string()))?;

        let total_size = HEADER_SIZE + capacity;

        let wide_name: Vec<u16> = OsStr::new(&mapping_name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let handle = unsafe {
            OpenFileMappingW(
                FILE_MAP_ALL_ACCESS.0,
                false,
                PCWSTR::from_raw(wide_name.as_ptr()),
            )
        }
        .map_err(|e| MemioError::OpenFailed(format!("OpenFileMappingW failed: {:?}", e)))?;

        // Map the view with known size
        let ptr = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, total_size) };

        if ptr.Value.is_null() {
            unsafe { let _ = CloseHandle(handle); }
            return Err(MemioError::MmapFailed);
        }

        Ok(Self {
            name: name.to_string(),
            mapping_name,
            handle,
            ptr: ptr.Value as *mut u8,
            total_size,
            capacity,
            owns_handle: false, // We don't own this - it's a secondary view
        })
    }
}

impl Drop for WindowsSharedMemoryRegion {
    fn drop(&mut self) {
        unsafe {
            let addr = MEMORY_MAPPED_VIEW_ADDRESS { Value: self.ptr as *mut _ };
            let _ = UnmapViewOfFile(addr);
            let _ = CloseHandle(self.handle);
        }
        // Only remove from registry if this region owns the handle (was created, not opened)
        if self.owns_handle {
            if let Ok(mut registry) = REGISTRY.lock() {
                registry.remove(&self.name);
            }
            if let Ok(mut active) = ACTIVE_REGIONS.lock() {
                active.remove(&self.name);
            }
        }
    }
}

impl SharedMemoryRegion for WindowsSharedMemoryRegion {
    fn capacity(&self) -> usize {
        self.capacity
    }

    fn info(&self) -> Result<SharedStateInfo, MemioError> {
        let header_slice = unsafe { std::slice::from_raw_parts(self.ptr, HEADER_SIZE) };
        let (version, length) = read_header(header_slice, self.capacity)
            .ok_or(MemioError::InvalidHeader)?;

        Ok(SharedStateInfo {
            name: self.name.clone(),
            path: None,
            fd: None,
            version,
            length,
            capacity: self.capacity,
        })
    }

    fn write(&mut self, version: u64, data: &[u8]) -> Result<SharedStateInfo, MemioError> {
        if data.len() > self.capacity {
            return Err(MemioError::DataTooLarge {
                data_len: data.len(),
                capacity: self.capacity,
            });
        }

        // Write data after header
        unsafe {
            let data_ptr = self.ptr.add(HEADER_SIZE);
            ptr::copy_nonoverlapping(data.as_ptr(), data_ptr, data.len());
        }

        // Write header
        let header_slice = unsafe { std::slice::from_raw_parts_mut(self.ptr, HEADER_SIZE) };
        write_header_unchecked(header_slice, version, data.len());

        Ok(SharedStateInfo {
            name: self.name.clone(),
            path: None,
            fd: None,
            version,
            length: data.len(),
            capacity: self.capacity,
        })
    }

    fn read(&self) -> Result<Vec<u8>, MemioError> {
        let header_slice = unsafe { std::slice::from_raw_parts(self.ptr, HEADER_SIZE) };
        let (_, length) = read_header(header_slice, self.capacity)
            .ok_or(MemioError::InvalidHeader)?;

        let mut data = vec![0u8; length];
        unsafe {
            let data_ptr = self.ptr.add(HEADER_SIZE);
            ptr::copy_nonoverlapping(data_ptr, data.as_mut_ptr(), length);
        }

        Ok(data)
    }

    unsafe fn data_ptr(&self) -> *const u8 {
        unsafe { self.ptr.add(HEADER_SIZE) }
    }

    unsafe fn data_ptr_mut(&mut self) -> *mut u8 {
        unsafe { self.ptr.add(HEADER_SIZE) }
    }
}

/// Factory for Windows memio regions.
#[derive(Debug, Default)]
pub struct WindowsSharedMemoryFactory;

impl WindowsSharedMemoryFactory {
    /// Creates a new WindowsSharedMemoryFactory.
    pub fn new() -> Self {
        Self
    }
}

impl SharedMemoryFactory for WindowsSharedMemoryFactory {
    type Region = WindowsSharedMemoryRegion;

    fn create(&self, name: &str, capacity: usize) -> Result<Self::Region, MemioError> {
        WindowsSharedMemoryRegion::create(name, capacity)
    }

    fn open(&self, name: &str) -> Result<Self::Region, MemioError> {
        WindowsSharedMemoryRegion::open(name)
    }

    fn list(&self) -> Vec<String> {
        REGISTRY
            .lock()
            .map(|r| r.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn exists(&self, name: &str) -> bool {
        REGISTRY
            .lock()
            .map(|r| r.contains_key(name))
            .unwrap_or(false)
    }

    fn remove(&self, name: &str) -> Result<(), MemioError> {
        if let Ok(mut registry) = REGISTRY.lock() {
            registry.remove(name);
        }
        if let Ok(mut active) = ACTIVE_REGIONS.lock() {
            active.remove(name);
        }
        Ok(())
    }
}

// ============================================================================
// Helper functions for the Tauri plugin
// ============================================================================

/// Lists all memio region names.
pub fn list_shared_regions() -> Vec<String> {
    REGISTRY
        .lock()
        .map(|r| r.keys().cloned().collect())
        .unwrap_or_default()
}

/// Creates a new memio region and keeps it alive.
pub fn create_shared_region(name: &str, capacity: usize) -> Result<(), String> {
    let region = WindowsSharedMemoryRegion::create(name, capacity)
        .map_err(|e| e.to_string())?;
    
    // Store the handle to keep it alive
    {
        let mut active = ACTIVE_REGIONS.lock().unwrap();
        active.insert(name.to_string(), RegionHandle {
            handle: region.handle,
            ptr: region.ptr,
            total_size: region.total_size,
            capacity: region.capacity,
        });
    }
    
    // Prevent Drop from running (which would close the handle)
    std::mem::forget(region);
    
    tracing::info!("Created and stored shared region: {}", name);
    Ok(())
}

/// Reads data from memio region.
pub fn read_from_shared(name: &str) -> Result<(u64, Vec<u8>), String> {
    // First check if we have an active region
    let active = ACTIVE_REGIONS.lock().unwrap();
    if let Some(handle) = active.get(name) {
        // Read directly from our stored handle
        let header_slice = unsafe { std::slice::from_raw_parts(handle.ptr, HEADER_SIZE) };
        let (version, length) = read_header(header_slice, handle.capacity)
            .ok_or("Invalid header")?;
        
        let mut data = vec![0u8; length];
        unsafe {
            let data_ptr = handle.ptr.add(HEADER_SIZE);
            ptr::copy_nonoverlapping(data_ptr, data.as_mut_ptr(), length);
        }
        
        return Ok((version, data));
    }
    drop(active);
    
    // Fallback to opening
    let region = WindowsSharedMemoryRegion::open(name)
        .map_err(|e| e.to_string())?;
    
    let info = region.info().map_err(|e| e.to_string())?;
    let data = region.read().map_err(|e| e.to_string())?;
    
    Ok((info.version, data))
}

/// Writes data to memio region.
pub fn write_to_shared(name: &str, version: u64, data: &[u8]) -> Result<(), String> {
    // First check if we have an active region
    let active = ACTIVE_REGIONS.lock().unwrap();
    if let Some(handle) = active.get(name) {
        if data.len() > handle.capacity {
            return Err(format!("Data too large: {} > {}", data.len(), handle.capacity));
        }
        
        // Write data after header
        unsafe {
            let data_ptr = handle.ptr.add(HEADER_SIZE);
            ptr::copy_nonoverlapping(data.as_ptr(), data_ptr, data.len());
        }
        
        // Write header
        let header_slice = unsafe { std::slice::from_raw_parts_mut(handle.ptr, HEADER_SIZE) };
        write_header_unchecked(header_slice, version, data.len());
        
        return Ok(());
    }
    drop(active);
    
    // Fallback to opening
    let mut region = WindowsSharedMemoryRegion::open(name)
        .map_err(|e| e.to_string())?;
    
    region.write(version, data).map_err(|e| e.to_string())?;
    Ok(())
}

/// Gets the version of a memio region.
pub fn get_version(name: &str) -> Result<u64, String> {
    // First check if we have an active region
    let active = ACTIVE_REGIONS.lock().unwrap();
    if let Some(handle) = active.get(name) {
        let header_slice = unsafe { std::slice::from_raw_parts(handle.ptr, HEADER_SIZE) };
        let (version, _) = read_header(header_slice, handle.capacity)
            .ok_or("Invalid header")?;
        return Ok(version);
    }
    drop(active);
    
    // Fallback to opening
    let region = WindowsSharedMemoryRegion::open(name)
        .map_err(|e| e.to_string())?;
    
    let info = region.info().map_err(|e| e.to_string())?;
    Ok(info.version)
}

/// Checks if a memio region exists.
pub fn has_shared_region(name: &str) -> bool {
    REGISTRY
        .lock()
        .map(|r| r.contains_key(name))
        .unwrap_or(false)
}

/// Gets the raw pointer to a memio region's data area.
/// Returns the pointer to the data (after header).
pub fn get_shared_pointer(name: &str) -> Result<*mut u8, String> {
    let active = ACTIVE_REGIONS.lock().unwrap();
    if let Some(handle) = active.get(name) {
        // Return pointer to data area (after header)
        let data_ptr = unsafe { handle.ptr.add(HEADER_SIZE) };
        return Ok(data_ptr);
    }
    Err(format!("Region '{}' not found in active regions", name))
}

/// Gets the capacity (data size, excluding header) of a memio region.
pub fn get_shared_size(name: &str) -> Result<usize, String> {
    let active = ACTIVE_REGIONS.lock().unwrap();
    if let Some(handle) = active.get(name) {
        return Ok(handle.capacity);
    }
    drop(active);
    
    // Try from registry - stores (mapping_name, capacity)
    let registry = REGISTRY.lock().unwrap();
    if let Some((_mapping_name, capacity)) = registry.get(name) {
        return Ok(*capacity);
    }
    
    Err(format!("Region '{}' not found", name))
}