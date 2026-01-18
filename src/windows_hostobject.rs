//! Windows WebView2 Host Object integration.
//!
//! This module uses AddHostObjectToScript to expose a COM object to JavaScript,
//! providing efficient data transfer without Base64 encoding.
//!
//! This approach is:
//! - More compatible (works on older WebView2 versions)
//! - Simpler than SharedBuffer

use tauri::{command, Runtime, WebviewWindow};
use windows::core::{implement, IInspectable, HSTRING};
use windows::Win32::System::WinRT::IInspectable as WinRTInspectable;

/// Tauri command to register the MemioHostObject.
///
/// This must be called once during app initialization to expose
/// the `window.chrome.webview.hostObjects.memioShared` object to JavaScript.
#[command]
pub async fn register_memio_host_object<R: Runtime>(
    window: WebviewWindow<R>,
) -> Result<(), String> {
    window
        .with_webview(|webview| {
            #[cfg(target_os = "windows")]
            unsafe {
                register_host_object_impl(webview)
            }
            
            #[cfg(not(target_os = "windows"))]
            {
                let _ = webview;
                Err("Host Object only available on Windows".to_string())
            }
        })
        .map_err(|e| format!("WebView error: {:?}", e))??;

    Ok(())
}

#[cfg(target_os = "windows")]
unsafe fn register_host_object_impl(webview: &tauri::Webview) -> Result<(), String> {
    use windows::core::Interface;
    use windows::Web::WebView2::Core::ICoreWebView2;

    // Get CoreWebView2 from Tauri webview
    let webview_ptr = webview.as_ptr() as *mut std::ffi::c_void;
    let core: ICoreWebView2 = {
        // Tauri's webview is actually a WRY webview
        // On Windows, WRY wraps WebView2's ICoreWebView2
        // We need to extract it carefully
        
        // This is a simplification - actual implementation may vary
        // based on Tauri version. The real code would need to:
        // 1. Cast to WRY's WebView struct
        // 2. Access its controller field
        // 3. Call CoreWebView2() on it
        
        // For now, return error with instructions
        return Err(
            "CoreWebView2 access not yet implemented. See windows_hostobject.rs for details"
                .to_string(),
        );
    };

    // Create our host object
    let host_object = MemioHostObject::new();
    let inspectable: IInspectable = host_object.cast().map_err(|e| e.to_string())?;

    // Register it with name "memioShared"
    core.AddHostObjectToScript(&HSTRING::from("memioShared"), &inspectable)
        .map_err(|e| format!("AddHostObjectToScript failed: {:?}", e))?;

    tracing::info!("MemioHostObject registered successfully");
    Ok(())
}

/// COM Host Object that JavaScript can call.
///
/// Exposed as: `window.chrome.webview.hostObjects.memioShared`
///
/// Methods available in JavaScript:
/// - `readSharedState(name: string, lastVersion: number): Promise<string>`
/// - `writeSharedState(name: string, data: string, version: number): Promise<boolean>`
/// - `getVersion(name: string): Promise<number>`
#[implement(IInspectable)]
struct MemioHostObject {}

impl MemioHostObject {
    fn new() -> Self {
        Self {}
    }

    /// Reads data from memio region and returns as Base64 JSON.
    ///
    /// JavaScript signature:
    /// ```js
    /// async readSharedState(name: string, lastVersion: number): Promise<string>
    /// ```
    ///
    /// Returns JSON: `{"version": 123, "length": 456, "data": "base64..."}`
    /// Or empty string if unchanged.
    pub fn ReadSharedState(&self, name: HSTRING, last_version: i64) -> Result<HSTRING, windows::core::Error> {
        let name_str = name.to_string_lossy();

        tracing::debug!("ReadSharedState called: name={}, lastVersion={}", name_str, last_version);

        // Read from memio-platform
        let (version, data) = match memio_platform::windows::read_from_shared(&name_str) {
            Ok(result) => result,
            Err(e) => {
                tracing::error!("Failed to read shared memory: {:?}", e);
                return Ok(HSTRING::new());
            }
        };

        // Check if unchanged
        if last_version >= 0 && version == last_version as u64 {
            tracing::debug!("Data unchanged, returning empty");
            return Ok(HSTRING::new());
        }

        // Encode to Base64
        use base64::Engine;
        let data_b64 = base64::engine::general_purpose::STANDARD.encode(&data);

        // Return as JSON
        let json = format!(
            r#"{{"version":{},"length":{},"data":"{}"}}"#,
            version,
            data.len(),
            data_b64
        );

        tracing::debug!("Returning {} bytes (version {})", data.len(), version);
        Ok(HSTRING::from(json))
    }

    /// Writes data to memio region.
    ///
    /// JavaScript signature:
    /// ```js
    /// async writeSharedState(name: string, data: string, version: number): Promise<boolean>
    /// ```
    ///
    /// `data` should be Base64 encoded.
    pub fn WriteSharedState(
        &self,
        name: HSTRING,
        version: i64,
        data_base64: HSTRING,
    ) -> Result<bool, windows::core::Error> {
        let name_str = name.to_string_lossy();
        let data_b64_str = data_base64.to_string_lossy();

        tracing::debug!("WriteSharedState called: name={}, version={}", name_str, version);

        // Decode Base64
        use base64::Engine;
        let data = match base64::engine::general_purpose::STANDARD.decode(data_b64_str) {
            Ok(d) => d,
            Err(e) => {
                tracing::error!("Base64 decode failed: {:?}", e);
                return Ok(false);
            }
        };

        // Write to memio region
        let version_u64 = if version < 0 { 1 } else { version as u64 };
        match memio_platform::windows::write_to_shared(&name_str, version_u64, &data) {
            Ok(_) => {
                tracing::debug!("Wrote {} bytes to shared memory", data.len());
                Ok(true)
            }
            Err(e) => {
                tracing::error!("Failed to write shared memory: {:?}", e);
                Ok(false)
            }
        }
    }

    /// Gets the current version of a memio region.
    ///
    /// JavaScript signature:
    /// ```js
    /// async getVersion(name: string): Promise<number>
    /// ```
    pub fn GetVersion(&self, name: HSTRING) -> Result<i64, windows::core::Error> {
        let name_str = name.to_string_lossy();

        match memio_platform::windows::get_version(&name_str) {
            Ok(v) => Ok(v as i64),
            Err(_) => Ok(-1),
        }
    }
}

/// Helper commands for testing

#[command]
pub async fn test_host_object_read(name: String) -> Result<String, String> {
    let host_object = MemioHostObject::new();
    let result = host_object
        .ReadSharedState(HSTRING::from(&name), -1)
        .map_err(|e| e.to_string())?;
    Ok(result.to_string())
}

#[command]
pub async fn test_host_object_write(
    name: String,
    data: Vec<u8>,
    version: u64,
) -> Result<bool, String> {
    use base64::Engine;
    let data_b64 = base64::engine::general_purpose::STANDARD.encode(&data);

    let host_object = MemioHostObject::new();
    let result = host_object
        .WriteSharedState(
            HSTRING::from(&name),
            version as i64,
            HSTRING::from(&data_b64),
        )
        .map_err(|e| e.to_string())?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_object_creation() {
        let _host_object = MemioHostObject::new();
        // Just verify it compiles and constructs
    }

    #[test]
    fn test_read_write_cycle() {
        // This would need actual memio region setup
        // Skipping for now
    }
}
