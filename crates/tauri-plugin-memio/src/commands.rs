//! Unified Memio commands for Linux.
//!
//! These commands provide a unified API for:
//! - `memio_upload`: Upload file from URI/path to shared memory
//! - `memio_read`: Read data from shared memory buffer
//!
//! The implementation uses Linux shared memory.

use serde::{Deserialize, Serialize};
use tauri::{command, AppHandle, Runtime, Manager};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadResult {
    pub success: bool,
    pub bytes_written: usize,
    pub version: i64,
    pub duration_ms: f64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadResult {
    pub success: bool,
    pub version: i64,
    pub length: usize,
}

/// Upload a file to shared memory buffer.
///
/// # Arguments
/// - `buffer_name`: Name of the shared memory buffer
/// - `file_uri`: URI or path to the file
#[command]
pub async fn memio_upload<R: Runtime>(
    app: AppHandle<R>,
    #[allow(non_snake_case)] bufferName: String,
    #[allow(non_snake_case)] fileUri: String,
) -> Result<UploadResult, String> {
    let start = std::time::Instant::now();

    use memio_platform::MemioManager;

    let file_path = if fileUri.starts_with("file://") {
        fileUri.strip_prefix("file://").unwrap_or(&fileUri)
    } else {
        &fileUri
    };

    let data = std::fs::read(file_path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let manager = app
        .try_state::<std::sync::Arc<MemioManager>>()
        .ok_or("MemioManager not available")?;

    let version = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(1);

    manager
        .write(&bufferName, version, &data)
        .map_err(|e| format!("Failed to write to shared memory: {:?}", e))?;

    Ok(UploadResult {
        success: true,
        bytes_written: data.len(),
        version: version as i64,
        duration_ms: start.elapsed().as_secs_f64() * 1000.0,
    })
}

/// Read data from shared memory buffer.
///
/// # Arguments
/// - `buffer_name`: Name of the shared memory buffer
/// - `last_version`: Optional - skip read if version hasn't changed
///
/// # Returns
/// ReadResult with success, version, and length.
/// The actual data is read from shared memory by the frontend.
#[command]
pub fn memio_read<R: Runtime>(
    app: AppHandle<R>,
    #[allow(non_snake_case)] bufferName: String,
    #[allow(non_snake_case)] lastVersion: Option<i64>,
) -> Result<ReadResult, String> {
    use memio_platform::MemioManager;
    
    let manager = app.try_state::<std::sync::Arc<MemioManager>>()
        .ok_or("MemioManager not available")?;
    
    let result = manager.read(&bufferName)
        .map_err(|e| format!("Failed to read from shared memory: {:?}", e))?;
    
    // Check if version changed
    if let Some(last) = lastVersion {
        if result.version as i64 <= last {
            return Ok(ReadResult {
                success: false,
                version: result.version as i64,
                length: 0,
            });
        }
    }
    
    Ok(ReadResult {
        success: true,
        version: result.version as i64,
        length: result.data.len(),
    })
}
