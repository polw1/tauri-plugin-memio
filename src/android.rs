use serde::{Deserialize, Serialize};
use tauri::plugin::{PluginApi, PluginHandle};
use tauri::Runtime;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadRequest {
    pub buffer_name: String,
    pub file_uri: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadResponse {
    pub success: bool,
    pub bytes_written: usize,
    pub version: i64,
    pub duration_ms: f64,
    pub read_ms: f64,
    pub write_ms: f64,
}

pub struct MemioAndroid<R: Runtime>(pub PluginHandle<R>);

impl<R: Runtime> MemioAndroid<R> {
    pub fn upload_file_from_uri(
        &self,
        buffer_name: String,
        file_uri: String,
    ) -> Result<UploadResponse, Box<dyn std::error::Error>> {
        // Send both camelCase and snake_case to be resilient to bridge differences
        let payload = serde_json::json!({
            "bufferName": buffer_name,
            "fileUri": file_uri,
            "buffer_name": buffer_name,
            "file_uri": file_uri,
        });

        self.0
            .run_mobile_plugin("uploadFileFromUri", payload)
            .map_err(Into::into)
    }
}

pub(crate) fn register_android_plugin<R: Runtime>(
    api: PluginApi<R, ()>,
) -> Result<PluginHandle<R>, Box<dyn std::error::Error>> {
    api.register_android_plugin("app.tauri.memio", "MemioPlugin")
        .map_err(Into::into)
}
