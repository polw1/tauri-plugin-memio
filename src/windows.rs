//! Windows memio region integration for Tauri.
//!
//! This module provides Tauri commands for data transfer
//! using WebView2's SharedBuffer API.
//!
//! Upload (Front→Back):
//! 1. JS calls prepare_upload_buffer → Rust creates SharedBuffer
//! 2. Rust posts buffer to JS with ReadWrite access
//! 3. JS writes data to the ArrayBuffer
//! 4. JS calls commit_upload_buffer → Rust reads from SharedBuffer
//!
//! Download (Back→Front):
//! 1. JS calls send_download_buffer
//! 2. Rust reads data, creates SharedBuffer, writes data
//! 3. Rust posts buffer to JS with ReadOnly access
//! 4. JS reads data from the ArrayBuffer

use tauri::{command, Runtime, WebviewWindow};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};

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
// Upload Stream (Control Ring)
// ============================================================================

const CONTROL_HEADER_SIZE: usize = 16;
const CONTROL_ENTRY_SIZE: usize = 24;

struct UploadSession {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
    control_name: String,
    buffer_names: Vec<String>,
    version: u64,
    total_length: usize,
    capacity: u32,
}

static UPLOAD_SESSIONS: OnceLock<Mutex<HashMap<String, UploadSession>>> = OnceLock::new();

fn upload_sessions() -> &'static Mutex<HashMap<String, UploadSession>> {
    UPLOAD_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartUploadStreamArgs {
    pub name: String,
    pub total_length: usize,
    pub chunk_size: usize,
    pub buffer_count: u32,
    pub version: u64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartUploadStreamResponse {
    pub control_name: String,
    pub buffer_names: Vec<String>,
    pub capacity: u32,
    pub entry_size: u32,
}

/// Starts a ring-based upload stream using SharedBuffers with a control queue.
#[command]
pub async fn start_upload_stream<R: Runtime>(
    window: WebviewWindow<R>,
    args: StartUploadStreamArgs,
) -> Result<StartUploadStreamResponse, String> {
    use std::sync::mpsc;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Environment12, ICoreWebView2_17, ICoreWebView2_2,
        COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE,
    };
    use windows::core::PCWSTR;
    use windows_core::Interface;

    if args.buffer_count == 0 {
        return Err("buffer_count must be > 0".to_string());
    }

    let control_name = format!("{}__ctrl", args.name);
    let buffer_names: Vec<String> = (0..args.buffer_count)
        .map(|i| format!("{}__data_{}", args.name, i))
        .collect();
    let capacity = args.buffer_count;
    let control_size = CONTROL_HEADER_SIZE + (capacity as usize * CONTROL_ENTRY_SIZE);

    {
        let sessions = upload_sessions().lock().unwrap();
        if sessions.contains_key(&args.name) {
            return Err(format!("Upload stream '{}' already started", args.name));
        }
    }

    let (tx, rx) = mpsc::channel::<Result<StartUploadStreamResponse, String>>();
    let control_name_clone = control_name.clone();
    let buffer_names_clone = buffer_names.clone();
    let chunk_size = args.chunk_size;
    let capacity_clone = capacity;

    let webview_result = window.with_webview(move |webview| {
        let result: Result<StartUploadStreamResponse, String> = (|| {
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

                let webview17: ICoreWebView2_17 = core_webview
                    .cast()
                    .map_err(|e| format!("Failed to cast to WebView2_17: {:?}", e))?;

                // Control buffer
                let (_ctrl_ptr, _ctrl_size) = crate::windows_shared_buffer::create_shared_buffer(
                    &env12,
                    &control_name_clone,
                    control_size as u64,
                )
                .map_err(|e| format!("Failed to create control buffer: {:?}", e))?;

                // Initialize control header [head, tail, capacity, entry_size]
                let mut header = [0u8; CONTROL_HEADER_SIZE];
                header[8..12].copy_from_slice(&capacity_clone.to_le_bytes());
                header[12..16].copy_from_slice(&(CONTROL_ENTRY_SIZE as u32).to_le_bytes());
                crate::windows_shared_buffer::write_to_buffer(&control_name_clone, 0, &header)
                    .map_err(|e| format!("Failed to init control header: {e}"))?;

                let ctrl_metadata = format!(
                    r#"{{"name":"{}","size":{},"forUploadControl":true,"capacity":{},"entrySize":{}}}"#,
                    control_name_clone, control_size, capacity_clone, CONTROL_ENTRY_SIZE
                );
                let ctrl_wide: Vec<u16> =
                    ctrl_metadata.encode_utf16().chain(std::iter::once(0)).collect();
                {
                    let registry = crate::windows_shared_buffer::get_registry_internal();
                    let reg = registry.lock().unwrap();
                    let entry = reg
                        .get(&control_name_clone)
                        .ok_or_else(|| format!("Control buffer '{}' not found", control_name_clone))?;
                    webview17
                        .PostSharedBufferToScript(
                            &entry.buffer,
                            COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE,
                            PCWSTR::from_raw(ctrl_wide.as_ptr()),
                        )
                        .map_err(|e| format!("Failed to post control buffer: {:?}", e))?;
                }

                // Data buffers
                for buffer_name in buffer_names_clone.iter() {
                    let (_ptr, _actual_size) = crate::windows_shared_buffer::create_shared_buffer(
                        &env12,
                        buffer_name,
                        chunk_size as u64,
                    )
                    .map_err(|e| format!("Failed to create data buffer: {:?}", e))?;

                    let metadata = format!(
                        r#"{{"name":"{}","size":{},"forUpload":true}}"#,
                        buffer_name, chunk_size
                    );
                    let wide: Vec<u16> =
                        metadata.encode_utf16().chain(std::iter::once(0)).collect();
                    let registry = crate::windows_shared_buffer::get_registry_internal();
                    let reg = registry.lock().unwrap();
                    let entry = reg
                        .get(buffer_name)
                        .ok_or_else(|| format!("Buffer '{}' not found in registry", buffer_name))?;
                    webview17
                        .PostSharedBufferToScript(
                            &entry.buffer,
                            COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE,
                            PCWSTR::from_raw(wide.as_ptr()),
                        )
                        .map_err(|e| format!("Failed to post data buffer: {:?}", e))?;
                }

                Ok(StartUploadStreamResponse {
                    control_name: control_name_clone.clone(),
                    buffer_names: buffer_names_clone.clone(),
                    capacity: capacity_clone,
                    entry_size: CONTROL_ENTRY_SIZE as u32,
                })
            }
        })();

        let _ = tx.send(result);
    });

    if let Err(e) = webview_result {
        return Err(format!("with_webview failed: {:?}", e));
    }

    let response = rx
        .recv()
        .map_err(|e| format!("Failed to receive result: {:?}", e))??;

    // Start worker thread
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_thread = Arc::clone(&stop_flag);
    let buffer_names_thread = buffer_names.clone();
    let control_name_thread = control_name.clone();
    let name_thread = args.name.clone();
    let version = args.version;
    let total_length = args.total_length;
    let capacity_usize = capacity as usize;

    let handle = std::thread::spawn(move || {
        let stop_flag = stop_flag_thread;
        let mut data_ptrs: Vec<(*mut u8, u64)> = Vec::new();
        for buffer_name in buffer_names_thread.iter() {
            if let Ok(ptr) = crate::windows_shared_buffer::get_buffer_ptr(buffer_name) {
                data_ptrs.push(ptr);
            } else {
                return;
            }
        }
        let (ctrl_ptr, _ctrl_size) =
            match crate::windows_shared_buffer::get_buffer_ptr(&control_name_thread) {
                Ok(ptr) => ptr,
                Err(_) => return,
            };

        unsafe fn read_u32(ptr: *mut u8, offset: usize) -> u32 {
            let mut bytes = [0u8; 4];
            std::ptr::copy_nonoverlapping(ptr.add(offset), bytes.as_mut_ptr(), 4);
            u32::from_le_bytes(bytes)
        }
        unsafe fn write_u32(ptr: *mut u8, offset: usize, value: u32) {
            let bytes = value.to_le_bytes();
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.add(offset), 4);
        }
        unsafe fn read_u64(ptr: *mut u8, offset: usize) -> u64 {
            let mut bytes = [0u8; 8];
            std::ptr::copy_nonoverlapping(ptr.add(offset), bytes.as_mut_ptr(), 8);
            u64::from_le_bytes(bytes)
        }

        loop {
            let head = unsafe { read_u32(ctrl_ptr, 0) };
            let tail = unsafe { read_u32(ctrl_ptr, 4) };

            if head == tail {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
                continue;
            }

            let index = (head as usize) % capacity_usize;
            let entry_offset = CONTROL_HEADER_SIZE + index * CONTROL_ENTRY_SIZE;
            let buffer_index = unsafe { read_u32(ctrl_ptr, entry_offset) } as usize;
            let length = unsafe { read_u32(ctrl_ptr, entry_offset + 4) } as usize;
            let offset = unsafe { read_u64(ctrl_ptr, entry_offset + 8) } as usize;
            let finalize = unsafe { read_u32(ctrl_ptr, entry_offset + 16) } != 0;

            if buffer_index < data_ptrs.len() {
                let (data_ptr, data_size) = data_ptrs[buffer_index];
                if length <= data_size as usize {
                    let final_info = if finalize {
                        Some((version, total_length))
                    } else {
                        None
                    };
                    let _ = memio_platform::windows::write_chunk_from_ptr(
                        &name_thread,
                        data_ptr,
                        offset,
                        length,
                        final_info,
                    );
                }
            }

            let next_head = head.wrapping_add(1);
            unsafe { write_u32(ctrl_ptr, 0, next_head) };
        }
    });

    let mut sessions = upload_sessions().lock().unwrap();
    sessions.insert(
        args.name,
        UploadSession {
            stop: stop_flag,
            handle: Some(handle),
            control_name,
            buffer_names,
            version: args.version,
            total_length: args.total_length,
            capacity,
        },
    );

    Ok(response)
}

/// Stops a ring-based upload stream and releases SharedBuffers.
#[command]
pub async fn stop_upload_stream(name: String) -> Result<(), String> {
    let mut session = {
        let mut sessions = upload_sessions().lock().unwrap();
        sessions
            .remove(&name)
            .ok_or_else(|| format!("Upload stream '{}' not found", name))?
    };

    session.stop.store(true, Ordering::Relaxed);
    if let Some(handle) = session.handle.take() {
        let _ = handle.join();
    }

    let _ = crate::windows_shared_buffer::close_buffer(&session.control_name);
    for buffer_name in session.buffer_names.iter() {
        let _ = crate::windows_shared_buffer::close_buffer(buffer_name);
    }

    Ok(())
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

/// Prepare a SharedBuffer for upload.
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

#[derive(serde::Deserialize)]
pub struct CommitUploadArgs {
    pub name: String,
    pub version: u64,
    pub length: usize,
    pub offset: Option<usize>,
    #[serde(alias = "bufferName")]
    pub buffer_name: Option<String>,
    #[serde(alias = "totalLength")]
    pub total_length: Option<usize>,
    pub finalize: Option<bool>,
}

/// Commit an upload - read data from SharedBuffer and write to memio region.
/// If offset is provided, writes a chunk and optionally finalizes the header.
#[command]
pub async fn commit_upload_buffer(args: CommitUploadArgs) -> Result<(), String> {
    let shared_name = args.buffer_name.as_deref().unwrap_or(&args.name);
    let (ptr, size) = crate::windows_shared_buffer::get_buffer_ptr(shared_name)?;
    if args.length > size as usize {
        return Err(format!(
            "Requested length {} exceeds SharedBuffer size {} for '{}'",
            args.length, size, args.name
        ));
    }

    if let Some(offset) = args.offset {
        let finalize = args.finalize.unwrap_or(false);
        let final_info = if finalize {
            Some((
                args.version,
                args.total_length
                    .ok_or_else(|| "totalLength is required to finalize")?,
            ))
        } else {
            None
        };

        memio_platform::windows::write_chunk_from_ptr(
            &args.name,
            ptr,
            offset,
            args.length,
            final_info,
        )?;

        tracing::info!(
            "[MemioWindows] Upload chunk committed: {} bytes to '{}' (offset {}, finalize {})",
            args.length,
            args.name,
            offset,
            finalize
        );

        return Ok(());
    }

    let data_slice = unsafe { std::slice::from_raw_parts(ptr, args.length) };

    tracing::info!(
        "[MemioWindows] Upload committed: {} bytes to '{}' (v{})",
        data_slice.len(),
        args.name,
        args.version
    );

    memio_platform::windows::write_to_shared(&args.name, args.version, data_slice)?;

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

/// Send data via SharedBuffer (back→front).
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

    // Read header info without copying the payload
    let (version, data_len) = memio_platform::windows::read_shared_info(&name)?;

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

                let (ptr, _actual_size) = crate::windows_shared_buffer::create_shared_buffer(
                    &env12, &buffer_name, data_len as u64
                ).map_err(|e| format!("Failed to create SharedBuffer: {:?}", e))?;

                // Write data to buffer BEFORE posting to JS (single copy)
                memio_platform::windows::copy_shared_to_ptr(&name_clone, ptr, data_len)
                    .map_err(|e| format!("Failed to copy to SharedBuffer: {:?}", e))?;

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
