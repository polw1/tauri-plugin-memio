# Memio Windows Architecture

This document describes the communication architecture between the Rust backend and the JavaScript frontend on Windows.

## Overview

The Windows implementation uses:
- **READ (Back→Front)**: WebView2 SharedBuffer API with `PostSharedBufferToScript`
- **WRITE (Front→Back)**: WebView2 SharedBuffer API with direct memory access
  - Small writes: single SharedBuffer + commit_upload_buffer`r
  - Large writes: control-ring stream (SharedBuffers + control queue)

Unlike Android and Linux, Windows uses **WebView2's native SharedBuffer API** for data transfer between Rust and JavaScript.

---

## READ: Backend → Frontend (Rust → JavaScript)

### Data Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              RUST BACKEND                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  // src/windows.rs                                                          │
│                                                                             │
│  send_download_buffer(window, name)                                         │
│         │                                                                   │
│         ▼                                                                   │
│  ┌─────────────────────────────────────────┐                                │
│  │  1. Read data from memio-platform       │                                │
│  │     memio_platform::windows::           │                                │
│  │         read_from_shared(&name)         │                                │
│  │                                         │                                │
│  │  2. Create WebView2 SharedBuffer        │                                │
│  │     ICoreWebView2Environment12::        │                                │
│  │         CreateSharedBuffer(size)        │                                │
│  │                                         │                                │
│  │  3. Get buffer pointer                  │                                │
│  │     buffer.Buffer(&mut ptr)             │                                │
│  │                                         │                                │
│  │  4. Write data to SharedBuffer          │                                │
│  │     windows_shared_buffer::             │                                │
│  │         write_to_buffer(name, 0, data)  │                                │
│  │                                         │                                │
│  │  5. Post to JavaScript                  │                                │
│  │     ICoreWebView2_17::                  │                                │
│  │         PostSharedBufferToScript(       │                                │
│  │             buffer,                     │                                │
│  │             READ_ONLY,                  │                                │
│  │             metadata_json               │                                │
│  │         )                               │                                │
│  └─────────────────────────────────────────┘                                │
│                                                                             │
│  Metadata JSON sent with buffer:                                            │
│  {                                                                          │
│      "name": "state",                                                       │
│      "bufferName": "download_state",                                        │
│      "version": 42,                                                         │
│      "size": 1024,                                                          │
│      "forDownload": true                                                    │
│  }                                                                          │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
             │
             │ WebView2 SharedBuffer (memory mapping)
             ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                            JAVASCRIPT FRONTEND                              │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  // Listen for SharedBuffer events                                          │
│                                                                             │
│  chrome.webview.addEventListener('sharedbufferreceived', (event) => {       │
│      const { additionalData, getBuffer } = event;                           │
│      const metadata = JSON.parse(additionalData);                           │
│                                                                             │
│      if (metadata.forDownload) {                                            │
│          // Get ArrayBuffer (direct view of memio buffer)                   │
│          const arrayBuffer = getBuffer();                                   │
│          const data = new Uint8Array(arrayBuffer);                          │
│                                                                             │
│          // Use data directly - no copy needed!                             │
│          processDownloadedData(metadata.name, data, metadata.version);      │
│      }                                                                      │
│  });                                                                        │
│                                                                             │
│  // Request download from Rust                                              │
│  async function requestDownload(name) {                                     │
│      await invoke('plugin:memio|send_download_buffer', { name });           │
│      // Data arrives via sharedbufferreceived event                         │
│  }                                                                          │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```
---

## WRITE: Frontend → Backend (JavaScript → Rust)

### Small Writes (single buffer)

### Data Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                            JAVASCRIPT FRONTEND                              │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  // Step 1: Request upload buffer from Rust                                 │
│                                                                             │
│  async function uploadData(name, data, version) {                           │
│      // Prepare buffer - Rust creates SharedBuffer and posts it to JS       │
│      const response = await invoke('plugin:memio|prepare_upload_buffer', {  │
│          name,                                                              │
│          size: data.byteLength                                              │
│      });                                                                    │
│                                                                             │
│      // Buffer arrives via sharedbufferreceived event                       │
│      // Wait for it...                                                      │
│  }                                                                          │
│                                                                             │
│  // Step 2: Receive buffer and write data                                   │
│                                                                             │
│  let pendingUpload = null;                                                  │
│                                                                             │
│  chrome.webview.addEventListener('sharedbufferreceived', (event) => {       │
│      const { additionalData, getBuffer } = event;                           │
│      const metadata = JSON.parse(additionalData);                           │
│                                                                             │
│      if (metadata.forUpload) {                                              │
│          // Get ArrayBuffer with ReadWrite access                           │
│          const arrayBuffer = getBuffer();                                   │
│          const view = new Uint8Array(arrayBuffer);                          │
│                                                                             │
│          // Write data directly to memio region                             │
│          view.set(pendingUpload.data);                                      │
│                                                                             │
│          // Notify Rust to commit the upload                                │
│          invoke('plugin:memio|commit_upload_buffer', {                      │
│              name: metadata.name,                                           │
│              version: pendingUpload.version,                                │
│              length: pendingUpload.data.byteLength                          │
│          });                                                                │
│      }                                                                      │
│  });                                                                        │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
             │
             │ WebView2 SharedBuffer (memory mapping)
             ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                              RUST BACKEND                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  // src/windows.rs                                                          │
│                                                                             │
│  ┌─────────────────────────────────────────┐                                │
│  │  prepare_upload_buffer(window, name,    │                                │
│  │                        size)            │                                │
│  │         │                               │                                │
│  │         ▼                               │                                │
│  │  1. Create WebView2 SharedBuffer        │                                │
│  │     ICoreWebView2Environment12::        │                                │
│  │         CreateSharedBuffer(size)        │                                │
│  │                                         │                                │
│  │  2. Register in global registry         │                                │
│  │     SHARED_BUFFERS.insert(name, entry)  │                                │
│  │                                         │                                │
│  │  3. Post to JavaScript with ReadWrite   │                                │
│  │     ICoreWebView2_17::                  │                                │
│  │         PostSharedBufferToScript(       │                                │
│  │             buffer,                     │                                │
│  │             READ_WRITE,                 │                                │
│  │             metadata_json               │                                │
│  │         )                               │                                │
│  └─────────────────────────────────────────┘                                │
│                                                                             │
│  // After JS writes data and calls commit:                                  │
│                                                                             │
│  ┌─────────────────────────────────────────┐                                │
│  │  commit_upload_buffer(name, version,    │                                │
│  │                       length)           │                                │
│  │         │                               │                                │
│  │         ▼                               │                                │
│  │  1. Read data from SharedBuffer         │                                │
│  │     windows_shared_buffer::             │                                │
│  │         read_from_buffer(name, 0, len)  │                                │
│  │                                         │                                │
│  │  2. Write to memio-platform region      │                                │
│  │     memio_platform::windows::           │                                │
│  │         write_to_shared(name, ver, data)│                                │
│  └─────────────────────────────────────────┘                                │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```
---

### Large Writes (streaming with control ring)

For large files, JS avoids per-chunk IPC. Rust creates a control SharedBuffer
that acts as a ring queue of descriptors and spawns a worker thread to drain it.
JS runs the streaming loop in a Web Worker to avoid blocking the UI thread.

Flow:
1. JS calls start_upload_stream with { name, totalLength, chunkSize, bufferCount, version }.
2. Rust creates:
   - Control buffer ${name}__ctrl (header + ring entries)
   - Data buffers ${name}__data_{i} for ufferCount
   - Posts all buffers to JS.
3. JS worker writes each chunk into a data buffer and enqueues a descriptor:
   { bufferIndex, length, offset, finalize } into the control ring.
4. Rust worker reads the ring and calls
   memio_platform::windows::write_chunk_from_ptr(...) for each entry.
5. JS waits until head == tail and calls stop_upload_stream.

Control header (16 bytes):
`
offset  size  field
0       4     head (u32)
4       4     tail (u32)
8       4     capacity (u32)
12      4     entry_size (u32)
`

Control entry (24 bytes):
`
offset  size  field
0       4     buffer_index (u32)
4       4     length (u32)
8       8     offset (u64)
16      4     finalize (u32)
20      4     reserved (u32)
`
## Implementation Files

### Rust

| File | Responsibility |
|------|----------------|
| `src/windows.rs` | Tauri commands: `prepare_upload_buffer`, `commit_upload_buffer`, `send_download_buffer`, utility commands |
| `src/windows_shared_buffer.rs` | WebView2 SharedBuffer registry and operations |
| `memio-platform/src/windows.rs` | Windows memio regions (memory-mapped files) |

### TypeScript

| File | Responsibility |
|------|----------------|
| `memio-client/src/platform/windows.ts` | `hasWindowsSharedBuffer()`, SharedBuffer event handling |
| `memio-client/src/client.ts` | Unified client API |

---

## Component Diagram

```
┌──────────────────────────────────────────────────────────────────┐
│                         FRONTEND (WebView2)                      │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │                     memio-client                           │  │
│  │  ┌─────────────────┐        ┌─────────────────────────┐    │  │
│  │  │  READ           │        │  WRITE                  │    │  │
│  │  │  invoke(send_   │        │  invoke(prepare_        │    │  │
│  │  │  download_      │        │  upload_buffer)         │    │  │
│  │  │  buffer)        │        │  + commit_upload_buffer │    │  │
│  │  └────────┬────────┘        └───────────┬─────────────┘    │  │
│  └───────────┼─────────────────────────────┼──────────────────┘  │
│              │                             │                     │
│              │ sharedbufferreceived        │ sharedbufferreceived│
│              ▼                             ▼                     │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │              chrome.webview.addEventListener               │  │
│  │                   'sharedbufferreceived'                   │  │
│  │                                                            │  │
│  │      getBuffer() → ArrayBuffer (direct view)               │  │
│  └────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────┘
               │                             ▲
               │ WebView2 SharedBuffer       │
               ▼                             │
┌──────────────────────────────────────────────────────────────────┐
│                         RUST BACKEND                             │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │                    windows.rs (Tauri Commands)              │ │
│  │                                                             │ │
│  │  prepare_upload_buffer() ─────► Create SharedBuffer         │ │
│  │  commit_upload_buffer()  ─────► Read from SharedBuffer      │ │
│  │  send_download_buffer()  ─────► Write to SharedBuffer       │ │
│  │                                                             │ │
│  │  Utility commands:                                          │ │
│  │  create_shared_buffer_windows()                             │ │
│  │  list_shared_buffers_windows()                              │ │
│  │  has_shared_buffer()                                        │ │
│  │                                                             │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                              │                                   │
│                              ▼                                   │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │               windows_shared_buffer.rs                      │ │
│  │                                                             │ │
│  │    SHARED_BUFFERS: HashMap<String, SharedBufferEntry>       │ │
│  │                                                             │ │
│  │    SharedBufferEntry {                                      │ │
│  │        buffer: ICoreWebView2SharedBuffer,                   │ │
│  │        ptr: *mut u8,                                        │ │
│  │        size: u64,                                           │ │
│  │    }                                                        │ │
│  │                                                             │ │
│  │    create_shared_buffer() → Create and register             │ │
│  │    write_to_buffer()      → Write bytes to buffer           │ │
│  │    read_from_buffer()     → Read bytes from buffer          │ │
│  │    post_buffer_to_script()→ Send to JS                      │ │
│  │    close_buffer()         → Cleanup                         │ │
│  │                                                             │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                              │                                   │
│                              ▼                                   │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │            memio-platform/src/windows.rs                    │ │
│  │                                                             │ │
│  │    REGISTRY: HashMap<String, WindowsSharedMemoryRegion>     │ │
│  │                                                             │ │
│  │    create_shared_region()  → Create memory-mapped region    │ │
│  │    write_to_shared()       → Write data + header            │ │
│  │    read_from_shared()      → Read data + version            │ │
│  │    list_shared_regions()   → List all regions               │ │
│  │    has_shared_region()     → Check if exists                │ │
│  │                                                             │ │
│  └─────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────┘
```

---

## WebView2 SharedBuffer API

The Windows implementation uses WebView2's native SharedBuffer API, which provides:

### Key Interfaces

| Interface | Description |
|-----------|-------------|
| `ICoreWebView2Environment12` | `CreateSharedBuffer(size)` - Creates a new SharedBuffer |
| `ICoreWebView2SharedBuffer` | `Buffer(&mut ptr)` - Gets raw pointer to buffer memory |
| `ICoreWebView2_17` | `PostSharedBufferToScript(buffer, access, json)` - Posts buffer to JS |

### Access Modes

| Mode | Value | Description |
|------|-------|-------------|
| `COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY` | ReadOnly | JS can only read (for downloads) |
| `COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE` | ReadWrite | JS can read and write (for uploads) |

### JavaScript Event

```javascript
chrome.webview.addEventListener('sharedbufferreceived', (event) => {
    // event.additionalData - JSON metadata string
    // event.getBuffer()    - Returns ArrayBuffer (direct view)
});
```

---

## Metadata JSON Format

### Upload Buffer Metadata

```json
{
    "name": "state",
    "size": 1024,
    "forUpload": true
}
```

### Download Buffer Metadata

```json
{
    "name": "state",
    "bufferName": "download_state",
    "version": 42,
    "size": 1024,
    "forDownload": true
}
```

---

## Advantages Over Traditional IPC

| Aspect | Traditional Tauri IPC | WebView2 SharedBuffer |
|--------|----------------------|----------------------|
| Data Copy | Multiple copies (JS → JSON → Rust) | SharedBuffer (memio region) |
| Encoding | Base64 for binary data | Direct binary access |
| Overhead | High for large data | Minimal |
| Latency | Higher | Lower |
| Memory | 2-3x data size | 1x data size |

---

## Requirements

- **WebView2 Runtime**: Version 1.0.1661.34 or later (for SharedBuffer API)
- **Windows**: Windows 10 or later
- **Tauri**: With WebView2 feature enabled

---

## Header Layout (memio-platform)

The memio-platform Windows regions use the same header format as Linux:

```
Offset  Size   Field      Description
──────  ─────  ─────────  ──────────────────────────────
0       8      magic      Magic number: 0x4F425255545F4F54 ("MEMIO_OB")
8       8      version    Version number (u64 LE)
16      8      length     Data length in bytes (u64 LE)
24      N      data       Actual payload data
```

**Note**: The WebView2 SharedBuffer itself does not use headers - raw data is written directly. Headers are only used when storing data in memio-platform regions.

---

## References

- [WebView2 SharedBuffer API](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2environment12#createsharedbuffer) - Official documentation for `CreateSharedBuffer`.
- [PostSharedBufferToScript](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_17#postsharedbuffertoscript) - Send SharedBuffer to JavaScript.
- [webview2-com Crate](https://crates.io/crates/webview2-com) - Rust bindings for WebView2.
- [WebView2 Release Notes](https://learn.microsoft.com/en-us/microsoft-edge/webview2/release-notes) - Version history and feature availability.




