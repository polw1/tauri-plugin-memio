use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;
use tauri::Manager;

#[cfg(target_os = "android")]
pub mod android;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "windows")]
pub mod windows_shared_buffer;

mod commands;
pub use commands::{memio_upload, memio_read, UploadResult, ReadResult};

/// Initializes the Memio plugin.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    #[cfg(target_os = "linux")]
    {
        if let Err(err) = ensure_webkit_extension_dir() {
            eprintln!("Memio WebKit extension setup failed: {:?}", err);
        }
    }

    let builder = Builder::new("memio")
        .setup(move |app, api| {
            #[cfg(target_os = "android")]
            {
                match android::register_android_plugin(api) {
                    Ok(handle) => {
                        let memio_android = android::MemioAndroid(handle);
                        app.manage(memio_android);
                    }
                    Err(err) => {
                        eprintln!("Memio Android plugin registration failed: {:?}", err);
                    }
                }
            }
            #[cfg(target_os = "linux")]
            {
                if let Err(err) = linux::replace_webview_windows_with_extensions(app) {
                    eprintln!("Memio WebKit extension window update failed: {:?}", err);
                }
            }
            #[cfg(target_os = "windows")]
            {
                tracing::info!("[MemioWindows] SharedBuffer plugin initialized");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::memio_upload,
            commands::memio_read,
        ]);

    builder.build()
}

#[cfg(target_os = "linux")]
fn ensure_webkit_extension_dir() -> Result<(), String> {
    let _ = linux::resolve_webkit_extension_dir()?;
    Ok(())
}

/// Builds webview windows with the Memio WebKit extension on Linux.
#[cfg(target_os = "linux")]
pub fn build_webview_windows<R: Runtime>(app: &tauri::App<R>) -> Result<(), String> {
    let configs = app.config().app.windows.clone();
    let windows = app.webview_windows();
    for (_, window) in windows {
        let _ = window.close();
    }

    let extension_path = Some(linux::resolve_webkit_extension_dir()?);
    let init_script = linux::build_shared_paths_script();

    for window_config in configs.iter() {
        let mut builder = tauri::WebviewWindowBuilder::from_config(app.handle(), window_config)
            .map_err(|err| err.to_string())?;
        if let Some(path) = extension_path.as_ref() {
            builder = builder.extensions_path(path);
        }
        if let Some(script) = init_script.as_ref() {
            builder = builder.initialization_script(script);
        }
        builder.build().map_err(|err| err.to_string())?;
    }

    Ok(())
}
