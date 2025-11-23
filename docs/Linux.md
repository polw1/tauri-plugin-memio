# Linux Architecture

MemioTauri on Linux uses a WebKitGTK extension to expose shared memory buffers directly to the WebView.

## Data Flow

```
Rust (MemioManager)
  │
  ├── create_buffer() → /dev/shm + mmap
  │
  └── write/read header + payload

WebKit Extension (memio_web_extension.so)
  │
  ├── mmap /dev/shm files
  └── expose TypedArray views to JS

JavaScript
  ├── memioSharedBuffer(name)
  └── memioWriteSharedBuffer(name, data)
```

## WebKit Extension

The extension:
- Maps shared files into the WebView process.
- Exposes global helpers (`memioSharedBuffer`, `memioWriteSharedBuffer`).
- Updates a manifest (`__memioSharedManifest`) for discovery.

## Header Layout

The buffer header is defined in `shared/shared_state_spec.json` and generated into:
- `crates/memio-core/src/shared_state_spec.rs`
- `packages/memio-client/src/shared-state-spec.ts`
- `extensions/webkit-linux/memio_spec.h`

The payload starts immediately after the header.
