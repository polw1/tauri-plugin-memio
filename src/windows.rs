//! Windows memio region integration for Tauri.
//!
//! This module provides Tauri commands for direct data transfer
//! using WebView2's SharedBuffer API.
//!
//! Upload (Front→Back):
//! 1. JS calls prepare_upload_buffer → Rust creates SharedBuffer
//! 2. Rust posts buffer to JS with ReadWrite access
//! 3. JS writes data directly to the ArrayBuffer (direct)
//! 4. JS calls commit_upload_buffer → Rust reads from SharedBuffer
//!
//! Download (Back→Front):
//! 1. JS calls send_download_buffer
//! 2. Rust reads data, creates SharedBuffer, writes data
//! 3. Rust posts buffer to JS with ReadOnly access
//! 4. JS reads data from the ArrayBuffer (direct)

use tauri::{command, Runtime, WebviewWindow};

// ============================================================================
// Utility Commands
// ============================================================================

/// Creates a new Windows memio region.
#[command]
pub async fn create_shared_buffer_windows(name: String, size: usize) -> Result<(), String> {
    memio_platform::windows::create_shared_region(&name, size)
}

/// Lists all Windows memio regions.
#[command]
pub async fn list_shared_buffers_windows() -> Result<Vec<String>, String> {
    Ok(memio_platform::windows::list_shared_regions())
}

/// Checks if a memio region exists.
#[command]
pub async fn has_shared_buffer(name: String) -> Result<bool, String> {
    Ok(memio_platform::windows::has_shared_region(&name))
}

// ============================================================================
// Direct Upload (Front → Back)
// ============================================================================

/// Response from prepare_upload_buffer
#[derive(serde::Serialize)]
pub struct PrepareBufferResponse {
    pub name: String,
    pub size: u64,
    pub ready: bool,
}

/// Prepare a SharedBuffer for direct upload.
/// Creates a WebView2 SharedBuffer and posts it to JS.
#[command]
pub async fn prepare_upload_buffer<R: Runtime>(
    window: WebviewWindow<R>,
    name: String,
    size: u64,
) -> Result<PrepareBufferResponse, String> {
    use std::sync::mpsc;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Environment12, ICoreWebView2_17, ICoreWebView2_2,
        COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE,
    };
    use windows::core::PCWSTR;
    use windows_core::Interface;

    tracing::info!(
        "[MemioWindows] prepare_upload_buffer: '{}' ({} bytes)",
        name,
        size
    );

    let (tx, rx) = mpsc::channel::<Result<PrepareBufferResponse, String>>();
    let name_clone = name.clone();

    let webview_result = window.with_webview(move |webview| {
        let result: Result<PrepareBufferResponse, String> = (|| {
            let controller = webview.controller();
            unsafe {
                let core_webview = controller
                    .CoreWebView2()
                    .map_err(|e| format!("Failed to get CoreWebView2: {:?}", e))?;

                let webview2: ICoreWebView2_2 = core_webview
                    .cast()
                    .map_err(|e| format!("Failed to cast to WebView2_2: {:?}", e))?;

                let env = webview2
                    .Environment()
                    .map_err(|e| format!("Failed to get Environment: {:?}", e))?;

                let env12: ICoreWebView2Environment12 = env
                    .cast()
                    .map_err(|e| format!("Failed to cast to Environment12: {:?}", e))?;

                let (ptr, actual_size) =
                    crate::windows_shared_buffer::create_shared_buffer(&env12, &name_clone, size)
                        .map_err(|e| format!("Failed to create SharedBuffer: {:?}", e))?;

                let webview17: ICoreWebView2_17 = core_webview
                    .cast()
                    .map_err(|e| format!("Failed to cast to WebView2_17: {:?}", e))?;

                let metadata = format!(
                    r#"{{"name":"{}","size":{},"forUpload":true}}"#,
                    name_clone, actual_size
                );
                let wide: Vec<u16> = metadata.encode_utf16().chain(std::iter::once(0)).collect();

                {
                    let registry = crate::windows_shared_buffer::get_registry_internal();
                    let reg = registry.lock().unwrap();
                    if let Some(entry) = reg.get(&name_clone) {
                        webview17
                            .PostSharedBufferToScript(
                                &entry.buffer,
                                COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE,
                                PCWSTR::from_raw(wide.as_ptr()),
                            )
                            .map_err(|e| format!("Failed to post SharedBuffer: {:?}", e))?;
                    } else {
                        return Err(format!("Buffer '{}' not found in registry", name_clone));
                    }
                }

                tracing::info!(
                    "[MemioWindows] Upload buffer posted: '{}' ({} bytes at {:?})",
                    name_clone,
                    actual_size,
                    ptr
                );

                Ok(PrepareBufferResponse {
                    name: name_clone.clone(),
                    size: actual_size,
                    ready: true,
                })
            }
        })();

        let _ = tx.send(result);
    });

    if let Err(e) = webview_result {
        return Err(format!("with_webview failed: {:?}", e));
    }

    rx.recv()
        .map_err(|e| format!("Failed to receive result: {:?}", e))?
}

/// Commit an upload - read data from SharedBuffer and write to memio region.
#[command]
pub async fn commit_upload_buffer(name: String, version: u64, length: usize) -> Result<(), String> {
    let data = crate::windows_shared_buffer::read_from_buffer(&name, 0, length)?;

    tracing::info!(
        "[MemioWindows] Upload committed: {} bytes to '{}' (v{})",
        data.len(),
        name,
        version
    );

    memio_platform::windows::write_to_shared(&name, version, &data)?;

    Ok(())
}

// ============================================================================
// Direct Download (Back → Front)
// ============================================================================

/// Response for download buffer
#[derive(serde::Serialize)]
pub struct DownloadBufferResponse {
    pub name: String,
    pub version: u64,
    pub size: usize,
    pub posted: bool,
}

/// Send data via direct SharedBuffer (back→front).
/// Creates a WebView2 SharedBuffer, copies data from memio region, and posts it to JS.
#[command]
pub async fn send_download_buffer<R: Runtime>(
    window: WebviewWindow<R>,
    name: String,
) -> Result<DownloadBufferResponse, String> {
    use std::sync::mpsc;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Environment12, ICoreWebView2_17, ICoreWebView2_2,
        COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY,
    };
    use windows::core::PCWSTR;
    use windows_core::Interface;

    // Read data from the memio-platform region
    let (version, data) = memio_platform::windows::read_from_shared(&name)?;
    let data_len = data.len();

    tracing::info!(
        "[MemioWindows] send_download_buffer: '{}' v{} ({} bytes)",
        name,
        version,
        data_len
    );

    let (tx, rx) = mpsc::channel::<Result<DownloadBufferResponse, String>>();
    let name_clone = name.clone();

    let webview_result = window.with_webview(move |webview| {
        let result: Result<DownloadBufferResponse, String> = (|| {
            let controller = webview.controller();
            unsafe {
                let core_webview = controller.CoreWebView2()
                    .map_err(|e| format!("Failed to get CoreWebView2: {:?}", e))?;

                let webview2: ICoreWebView2_2 = core_webview.cast()
                    .map_err(|e| format!("Failed to cast to WebView2_2: {:?}", e))?;

                let env = webview2.Environment()
                    .map_err(|e| format!("Failed to get Environment: {:?}", e))?;

                let env12: ICoreWebView2Environment12 = env.cast()
                    .map_err(|e| format!("Failed to cast to Environment12: {:?}", e))?;

                // Use different buffer name to avoid collision
                let buffer_name = format!("download_{}", name_clone);

                let (_ptr, _actual_size) = crate::windows_shared_buffer::create_shared_buffer(
                    &env12, &buffer_name, data_len as u64
                ).map_err(|e| format!("Failed to create SharedBuffer: {:?}", e))?;

                // Write data to buffer BEFORE posting to JS
                crate::windows_shared_buffer::write_to_buffer(&buffer_name, 0, &data)
                    .map_err(|e| format!("Failed to write to SharedBuffer: {:?}", e))?;

                let webview17: ICoreWebView2_17 = core_webview.cast()
                    .map_err(|e| format!("Failed to cast to WebView2_17: {:?}", e))?;

                let metadata = format!(
                    r#"{{"name":"{}","bufferName":"{}","version":{},"size":{},"forDownload":true}}"#,
                    name_clone, buffer_name, version, data_len
                );
                let wide: Vec<u16> = metadata.encode_utf16().chain(std::iter::once(0)).collect();

                {
                    let registry = crate::windows_shared_buffer::get_registry_internal();
                    let reg = registry.lock().unwrap();
                    if let Some(entry) = reg.get(&buffer_name) {
                        webview17.PostSharedBufferToScript(
                            &entry.buffer,
                            COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY,
                            PCWSTR::from_raw(wide.as_ptr()),
                        ).map_err(|e| format!("Failed to post SharedBuffer: {:?}", e))?;
                    } else {
                        return Err(format!("Buffer '{}' not found in registry", buffer_name));
                    }
                }

                tracing::info!(
                    "[MemioWindows] Download buffer posted: '{}' ({} bytes)",
                    buffer_name, data_len
                );

                Ok(DownloadBufferResponse {
                    name: name_clone.clone(),
                    version,
                    size: data_len,
                    posted: true,
                })
            }
        })();

        let _ = tx.send(result);
    });

    if let Err(e) = webview_result {
        return Err(format!("with_webview failed: {:?}", e));
    }

    rx.recv()
        .map_err(|e| format!("Failed to receive result: {:?}", e))?
}
