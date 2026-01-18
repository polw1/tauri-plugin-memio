# MemioTauri Architecture

MemioTauri provides a memio region data plane between a Tauri Rust backend and a WebView frontend, plus a small control plane for commands and metadata. A memio region is the shared data area Memio creates between Rust and the WebView, implemented with platform-specific primitives. The goal is to avoid Base64 IPC for large payloads while keeping the integration minimal for client apps.

## High-level components

- **memio-core**: Shared state schema, header layout, and core types.
- **memio-platform**: Platform-specific memio region implementations (Linux, Android, Windows).
- **tauri-plugin-memio**: Tauri plugin that wires platform hooks and commands.
- **memio-client**: TypeScript client that abstracts platform reads/writes.
- **linux WebKit extension**: Direct mmap access on Linux via injected JS helpers.

## Data plane vs control plane

- **Data plane**: Memio buffers (direct where supported).
- **Control plane**: Tauri commands and small metadata messages.

## Data flow summary

### Backend -> Frontend

- Rust writes into a memio buffer via `MemioManager.write`.
- Frontend reads the buffer:
  - **Linux**: WebKit extension maps `/dev/shm` and injects `memioSharedBuffer`.
  - **Android**: WebView intercepts `memio://buffer/<name>` and serves bytes.

### Frontend -> Backend

- Frontend writes data:
  - **Linux**: WebKit extension writes directly to the mmap file.
  - **Android**: `memio_upload` command reads a `content://` URI via ContentResolver and writes to the memio region.

## Header layout

- **Linux**: 24 bytes (magic + version + length)
- **Android**: 16 bytes (length + version)

## Platform differences

- **Linux**: Uses WebKitGTK extension + direct mmap; no HTTP layer.
- **Android**: Uses `memio://` protocol and JNI for memio region access.
- **Windows**: Uses SharedBuffer APIs (see code in `src/`).

## Further reading

- `docs/Linux.md`
- `docs/Android.md`
- `docs/Building.md`
