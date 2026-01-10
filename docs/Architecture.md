# MemioTauri Architecture

MemioTauri provides a shared-memory data plane between a Tauri Rust backend and a WebView frontend, plus a small control plane for commands and metadata. The goal is to avoid Base64 IPC for large payloads while keeping the integration minimal for client apps.

## High-level components

- **memio-core**: Shared state schema, header layout, and core types.
- **memio-platform**: Platform-specific shared memory implementations (Linux, Android, Windows).
- **tauri-plugin-memio**: Tauri plugin that wires platform hooks and commands.
- **memio-client**: TypeScript client that abstracts platform reads/writes.
- **linux WebKit extension**: Direct mmap access on Linux via injected JS helpers.

## Data plane vs control plane

- **Data plane**: Shared memory buffers (zero-copy or near zero-copy).
- **Control plane**: Tauri commands and small metadata messages.

## Data flow summary

### Backend -> Frontend

- Rust writes into a shared buffer via `MemioManager.write`.
- Frontend reads the buffer:
  - **Linux**: WebKit extension maps `/dev/shm` and injects `memioSharedBuffer`.
  - **Android**: WebView intercepts `memio://buffer/<name>` and serves bytes.

### Frontend -> Backend

- Frontend writes data:
  - **Linux**: WebKit extension writes directly to the mmap file.
  - **Android**: `memio_upload` command reads a `content://` URI via ContentResolver and writes to shared memory.

## Header layout

- **Linux**: 24 bytes (magic + version + length)
- **Android**: 16 bytes (length + version)

## Platform differences

- **Linux**: Uses WebKitGTK extension + direct mmap; no HTTP layer.
- **Android**: Uses `memio://` protocol and JNI for shared memory access.
- **Windows**: Uses SharedBuffer APIs (see code in `crates/tauri-plugin-memio`).

## Further reading

- `docs/Linux.md`
- `docs/Android.md`
- `docs/Building.md`
