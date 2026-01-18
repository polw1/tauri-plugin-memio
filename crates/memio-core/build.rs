//! Build script that generates shared_state_spec.rs from the JSON spec.
//!
//! This ensures Rust and TypeScript always use the same header layout.

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=../../shared/shared_state_spec.json");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let spec_path = Path::new(&manifest_dir)
        .join("..")
        .join("..")
        .join("shared")
        .join("shared_state_spec.json");

    // Read and parse the spec
    let spec_content = match fs::read_to_string(&spec_path) {
        Ok(content) => content,
        Err(e) => {
            eprintln!(
                "Warning: Could not read shared_state_spec.json: {}. Using defaults.",
                e
            );
            return;
        }
    };

    let spec: serde_json::Value = match serde_json::from_str(&spec_content) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "Warning: Could not parse shared_state_spec.json: {}. Using defaults.",
                e
            );
            return;
        }
    };

    // Extract values from JSON
    let magic_hex = spec["magic_hex"].as_str().unwrap_or("0x545552424F534852");
    let header_size = spec["header_size"].as_u64().unwrap_or(64);
    let magic_offset = spec["offsets"]["magic"].as_u64().unwrap_or(0);
    let version_offset = spec["offsets"]["version"].as_u64().unwrap_or(8);
    let length_offset = spec["offsets"]["length"].as_u64().unwrap_or(16);
    let endianness = spec["endianness"].as_str().unwrap_or("little");

    // Generate Rust code
    let generated = format!(
        r#"// Auto-generated from shared/shared_state_spec.json
// DO NOT EDIT MANUALLY - changes will be overwritten by build.rs

/// Magic bytes identifying a MemioTauri shared state file ("MEMIOSHR" in ASCII)
pub const SHARED_STATE_MAGIC: u64 = {magic_hex};

/// Total header size in bytes
pub const SHARED_STATE_HEADER_SIZE: usize = {header_size};

/// Byte offset of the magic field within the header
pub const SHARED_STATE_MAGIC_OFFSET: usize = {magic_offset};

/// Byte offset of the version field within the header
pub const SHARED_STATE_VERSION_OFFSET: usize = {version_offset};

/// Byte offset of the length field within the header
pub const SHARED_STATE_LENGTH_OFFSET: usize = {length_offset};

/// Endianness of multi-byte fields
pub const SHARED_STATE_ENDIANNESS: &str = "{endianness}";
"#
    );

    // Write to OUT_DIR
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("shared_state_spec.rs");
    fs::write(&dest_path, generated).expect("Failed to write generated shared_state_spec.rs");
}
