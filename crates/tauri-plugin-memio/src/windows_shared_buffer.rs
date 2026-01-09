//! Windows WebView2 SharedBuffer API for zero-copy data transfer.
//!
//! This module uses WebView2's native SharedBuffer API for efficient
//! bidirectional data transfer between JavaScript and Rust.
//!
//! Key APIs:
//! - ICoreWebView2Environment12::CreateSharedBuffer - Create shared memory
//! - ICoreWebView2_17::PostSharedBufferToScript - Send buffer to JS
//!
//! JS receives data via `chrome.webview.addEventListener('sharedbufferreceived', ...)`

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2Environment12, ICoreWebView2SharedBuffer, ICoreWebView2_17,
    COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE,
};
use windows::core::PCWSTR;
use windows_core::Interface;

/// Global registry of shared buffers by name
static SHARED_BUFFERS: OnceLock<Mutex<HashMap<String, SharedBufferEntry>>> = OnceLock::new();

pub struct SharedBufferEntry {
    pub buffer: ICoreWebView2SharedBuffer,
    pub ptr: *mut u8,
    pub size: u64,
}

// Safety: The buffer pointer is only accessed through synchronized methods
unsafe impl Send for SharedBufferEntry {}
unsafe impl Sync for SharedBufferEntry {}

fn get_registry() -> &'static Mutex<HashMap<String, SharedBufferEntry>> {
    SHARED_BUFFERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Internal access to registry for other modules
pub fn get_registry_internal() -> &'static Mutex<HashMap<String, SharedBufferEntry>> {
    get_registry()
}

/// Create a WebView2 SharedBuffer and register it.
/// Returns the buffer pointer and size.
pub fn create_shared_buffer(
    environment: &ICoreWebView2Environment12,
    name: &str,
    size: u64,
) -> windows::core::Result<(*mut u8, u64)> {
    unsafe {
        let buffer = environment.CreateSharedBuffer(size)?;
        
        // Get the buffer pointer
        let mut buffer_ptr: *mut u8 = std::ptr::null_mut();
        buffer.Buffer(&mut buffer_ptr)?;
        
        tracing::info!(
            "[MemioSharedBuffer] Created buffer '{}': {} bytes at {:?}",
            name, size, buffer_ptr
        );
        
        // Register in our map
        let entry = SharedBufferEntry {
            buffer,
            ptr: buffer_ptr,
            size,
        };
        
        let mut registry = get_registry().lock().unwrap();
        registry.insert(name.to_string(), entry);
        
        Ok((buffer_ptr, size))
    }
}

/// Post a SharedBuffer to JavaScript.
/// JS will receive it via `sharedbufferreceived` event.
pub fn post_buffer_to_script(
    webview: &ICoreWebView2,
    name: &str,
    additional_data_json: Option<&str>,
) -> windows::core::Result<()> {
    let registry = get_registry().lock().unwrap();
    
    let entry = registry.get(name).ok_or_else(|| {
        windows::core::Error::new::<&str>(
            windows::core::HRESULT(-1),
            &format!("Buffer '{}' not found", name),
        )
    })?;
    
    unsafe {
        // Cast to ICoreWebView2_17 to access PostSharedBufferToScript
        let webview17: ICoreWebView2_17 = webview.cast()?;
        
        let json_pcwstr = if let Some(json) = additional_data_json {
            let wide: Vec<u16> = json.encode_utf16().chain(std::iter::once(0)).collect();
            PCWSTR::from_raw(wide.as_ptr())
        } else {
            PCWSTR::null()
        };
        
        webview17.PostSharedBufferToScript(
            &entry.buffer,
            COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE,
            json_pcwstr,
        )?;
        
        tracing::info!(
            "[MemioSharedBuffer] Posted buffer '{}' to script ({} bytes)",
            name, entry.size
        );
    }
    
    Ok(())
}

/// Read data from a SharedBuffer (after JS has written to it).
pub fn read_from_buffer(name: &str, offset: usize, length: usize) -> Result<Vec<u8>, String> {
    let registry = get_registry().lock().unwrap();
    
    let entry = registry.get(name).ok_or_else(|| {
        format!("Buffer '{}' not found", name)
    })?;
    
    if offset + length > entry.size as usize {
        return Err(format!(
            "Read out of bounds: offset {} + length {} > size {}",
            offset, length, entry.size
        ));
    }
    
    unsafe {
        let src = entry.ptr.add(offset);
        let mut data = vec![0u8; length];
        std::ptr::copy_nonoverlapping(src, data.as_mut_ptr(), length);
        Ok(data)
    }
}

/// Write data to a SharedBuffer (before sending to JS).
pub fn write_to_buffer(name: &str, offset: usize, data: &[u8]) -> Result<(), String> {
    let registry = get_registry().lock().unwrap();
    
    let entry = registry.get(name).ok_or_else(|| {
        format!("Buffer '{}' not found", name)
    })?;
    
    if offset + data.len() > entry.size as usize {
        return Err(format!(
            "Write out of bounds: offset {} + length {} > size {}",
            offset, data.len(), entry.size
        ));
    }
    
    unsafe {
        let dst = entry.ptr.add(offset);
        std::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
    }
    
    Ok(())
}

/// Get the raw pointer to a buffer for direct access.
pub fn get_buffer_ptr(name: &str) -> Result<(*mut u8, u64), String> {
    let registry = get_registry().lock().unwrap();
    
    let entry = registry.get(name).ok_or_else(|| {
        format!("Buffer '{}' not found", name)
    })?;
    
    Ok((entry.ptr, entry.size))
}

/// Close and remove a SharedBuffer.
pub fn close_buffer(name: &str) -> Result<(), String> {
    let mut registry = get_registry().lock().unwrap();
    
    if let Some(entry) = registry.remove(name) {
        unsafe {
            let _ = entry.buffer.Close();
        }
        tracing::info!("[MemioSharedBuffer] Closed buffer '{}'", name);
        Ok(())
    } else {
        Err(format!("Buffer '{}' not found", name))
    }
}

/// List all registered SharedBuffers.
pub fn list_buffers() -> Vec<String> {
    let registry = get_registry().lock().unwrap();
    registry.keys().cloned().collect()
}

/// Check if a buffer exists.
pub fn has_buffer(name: &str) -> bool {
    let registry = get_registry().lock().unwrap();
    registry.contains_key(name)
}
