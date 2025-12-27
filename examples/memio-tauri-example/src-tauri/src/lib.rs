use std::sync::Arc;
use tauri::Manager;

use memio::prelude::*;

static SAMPLE_EXCEL: &[u8] = include_bytes!("../../public/sample_sales_data.xlsx");

struct AppState {
    manager: Arc<MemioManager>,
}

#[tauri::command]
fn load_file_via_ipc() -> Result<Vec<u8>, String> {
    Ok(SAMPLE_EXCEL.to_vec())
}

#[tauri::command]
fn write_file_to_memio(state: tauri::State<'_, AppState>) -> Result<usize, String> {
    let version = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(1);

    state
        .manager
        .write("file_transfer", version, SAMPLE_EXCEL)
        .map_err(|e| format!("Failed to write file: {:?}", e))?;

    Ok(SAMPLE_EXCEL.len())
}

#[tauri::command]
fn upload_file_ipc(data: Vec<u8>) -> Result<usize, String> {
    Ok(data.len())
}

#[tauri::command]
fn read_upload(state: tauri::State<'_, AppState>) -> Result<usize, String> {
    let result = state
        .manager
        .read("upload")
        .map_err(|e| format!("Failed to read upload: {:?}", e))?;
    Ok(result.data.len())
}

pub fn run() {
    let manager = Arc::new(MemioManager::new().expect("Failed to create MemioManager"));

    manager
        .create_buffer("file_transfer", 8 * 1024 * 1024)
        .expect("Failed to create file_transfer buffer");
    manager
        .create_buffer("upload", 8 * 1024 * 1024)
        .expect("Failed to create upload buffer");

    tauri::Builder::default()
        .setup(|app| {
            memio::plugin::build_webview_windows(app).map_err(|err| {
                let boxed: Box<dyn std::error::Error> =
                    Box::new(std::io::Error::new(std::io::ErrorKind::Other, err));
                tauri::Error::Setup(boxed.into())
            })?;

            Ok(())
        })
        .manage(AppState {
            manager: manager.clone(),
        })
        .plugin(memio::plugin::init())
        .invoke_handler(tauri::generate_handler![
            load_file_via_ipc,
            write_file_to_memio,
            upload_file_ipc,
            read_upload,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
