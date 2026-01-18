use serde_json::to_string as json_string;
use std::path::Path;
use tauri::{AppHandle, Manager, Runtime};

/// Sets the WebKit extension directory environment variable.
///
/// This function is Linux-specific and configures the path where WebKitGTK
/// will look for web extensions.
///
/// # Errors
/// Returns an error if the directory does not exist.
pub fn set_webkit_extension_dir(path: impl AsRef<Path>) -> Result<(), String> {
    let path = path.as_ref();
    if !path.exists() {
        return Err(format!(
            "WebKit extension directory does not exist: {:?}",
            path
        ));
    }
    // SAFETY: This is a controlled environment variable set at startup
    unsafe {
        std::env::set_var("WEBKIT_WEB_EXTENSION_DIRECTORY", path);
    }
    Ok(())
}

pub(crate) fn resolve_webkit_extension_dir() -> Result<std::path::PathBuf, String> {
    use std::path::PathBuf;

    if std::env::var("WEBKIT_WEB_EXTENSION_DIRECTORY").is_ok() {
        if let Ok(value) = std::env::var("WEBKIT_WEB_EXTENSION_DIRECTORY") {
            eprintln!("Memio WebKit extension directory (env): {}", value);
        }
        return std::env::var("WEBKIT_WEB_EXTENSION_DIRECTORY")
            .map(PathBuf::from)
            .map_err(|err| err.to_string());
    }

    if let Ok(path) = std::env::var("MEMIO_WEBKIT_EXTENSION_DIR") {
        eprintln!("Memio WebKit extension directory (override): {}", path);
        let path = PathBuf::from(path);
        set_webkit_extension_dir(path.clone())?;
        return Ok(path);
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let default_path = manifest_dir
        .join("extensions")
        .join("webkit-linux")
        .join("build");
    if default_path.exists() {
        let so_path = default_path.join("memio_web_extension.so");
        eprintln!(
            "Memio WebKit extension directory (auto): {} (exists: {})",
            default_path.display(),
            so_path.exists()
        );
        set_webkit_extension_dir(default_path.clone())?;
        return Ok(default_path);
    }

    Err("WEBKIT_WEB_EXTENSION_DIRECTORY not set and default extension path missing.".to_string())
}

pub(crate) fn replace_webview_windows_with_extensions<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<(), String> {
    let path = resolve_webkit_extension_dir()?;
    let windows = app.webview_windows();
    if windows.is_empty() {
        return Ok(());
    }

    let configs = app.config().app.windows.clone();
    for (_, window) in windows {
        let _ = window.close();
    }

    for window_config in configs.iter().filter(|w| w.create) {
        let builder = tauri::WebviewWindowBuilder::from_config(app, window_config)
            .map_err(|err| err.to_string())?
            .extensions_path(&path);
        builder.build().map_err(|err| err.to_string())?;
    }

    Ok(())
}

pub(crate) fn build_shared_paths_script() -> Option<String> {
    let registry = std::env::var("MEMIO_SHARED_REGISTRY").ok();
    let shared_path = std::env::var("MEMIO_SHARED_PATH").ok();
    if registry.is_none() && shared_path.is_none() {
        return None;
    }

    let mut script = String::new();
    if let Some(path) = registry {
        if let Ok(value) = json_string(&path) {
            script.push_str("globalThis.__memioSharedRegistryPath = ");
            script.push_str(&value);
            script.push(';');
        }
    }
    if let Some(path) = shared_path {
        if let Ok(value) = json_string(&path) {
            script.push_str("globalThis.__memioSharedPath = ");
            script.push_str(&value);
            script.push(';');
        }
    }
    if script.is_empty() {
        None
    } else {
        Some(script)
    }
}
