fn main() {
    tauri_plugin::Builder::new(&[
        "memio_upload",
        "memio_read",
    ])
    .build();

    #[cfg(target_os = "linux")]
    {
        if let Err(err) = build_webkit_extension() {
            eprintln!("cargo:warning=Memio WebKit extension build failed: {err}");
        }
    }

}
}

#[cfg(target_os = "linux")]
fn build_webkit_extension() -> Result<(), String> {
    use std::path::PathBuf;
    use std::process::Command;

    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").map_err(|e| e.to_string())?);
    let extension_dir = manifest_dir
        .join("..")
        .join("..")
        .join("extensions")
        .join("webkit-linux");
    let meson_file = extension_dir.join("meson.build");
    if !meson_file.exists() {
        return Ok(());
    }

    println!("cargo:rerun-if-changed={}", meson_file.display());
    println!(
        "cargo:rerun-if-changed={}",
        extension_dir.join("extension.c").display()
    );

    let build_dir = extension_dir.join("build");
    if !build_dir.exists() {
        let status = Command::new("meson")
            .arg("setup")
            .arg("build")
            .current_dir(&extension_dir)
            .status()
            .map_err(|e| format!("failed to run meson setup: {e}"))?;
        if !status.success() {
            return Err("meson setup failed".to_string());
        }
    }

    println!(
        "cargo:warning=Memio WebKit extension build: compiling in {}",
        build_dir.display()
    );
    let status = Command::new("meson")
        .arg("compile")
        .arg("-C")
        .arg("build")
        .current_dir(&extension_dir)
        .status()
        .map_err(|e| format!("failed to run meson compile: {e}"))?;
    if !status.success() {
        return Err("meson compile failed".to_string());
    }

    Ok(())
}
