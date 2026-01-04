# Memio Android Architecture

This document describes the communication architecture between the Rust backend and the JavaScript frontend on Android.

## Overview

The Android implementation removes traditional Base64 serialization using:
- **READ (Back→Front)**: `memio://` protocol with direct shared memory access
- **WRITE (Front→Back)**: Native ContentResolver via the Tauri command

---

## READ: Backend → Frontend (Rust → JavaScript)

### Data Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              RUST BACKEND                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  MemioManager.write("state", version, data)                                 │
│         │                                                                   │
│         ▼                                                                   │
│  ┌─────────────────────────────────────────┐                                │
│  │     ASharedMemory (Android NDK)         │                                │
│  │  ┌───────────┬──────────────────────┐   │                                │
│  │  │  HEADER   │        DATA          │   │                                │
│  │  │ 16 bytes  │    (raw bytes)       │   │                                │
│  │  │ version+  │                      │   │                                │
│  │  │ length    │                      │   │                                │
│  │  └───────────┴──────────────────────┘   │                                │
│  │         ▲ mmap (zero-copy)              │                                │
│  └─────────┼───────────────────────────────┘                                │
│            │                                                                │
└────────────┼────────────────────────────────────────────────────────────────┘
             │
             │ JNI: nativeGetDirectBuffer() → DirectByteBuffer
             ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                              KOTLIN LAYER                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  MemioSharedMemory.getDirectBuffer(name)                                    │
│         │                                                                   │
│         │ Returns DirectByteBuffer (zero-copy view of mmap)                 │
│         ▼                                                                   │
│  MemioWebViewClient.shouldInterceptRequest()                                │
│         │                                                                   │
│         │ Intercepts: memio://buffer/{name}                                 │
│         ▼                                                                   │
│  serveMemioBuffer(name)                                                     │
│         │                                                                   │
│         │ 1. Get DirectByteBuffer                                           │
│         │ 2. Read version from offset 8 (Long)                              │
│         │ 3. Read length from offset 0 (Long)                               │
│         │ 4. Extract data bytes [16..16+length]                             │
│         │ 5. Build WebResourceResponse with headers:                        │
│         │    - X-Memio-Version: {version}                                   │
│         │    - X-Memio-Length: {length}                                     │
│         ▼                                                                   │
│  return WebResourceResponse(                                                │
│      mimeType = "application/octet-stream",                                 │
│      data = ByteArrayInputStream(bytes),                                    │
│      headers = { X-Memio-Version, X-Memio-Length }                          │
│  )                                                                          │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
             │
             │ HTTP Response (raw bytes + headers)
             ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                            JAVASCRIPT FRONTEND                              │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  // memio-client/src/android.ts                                             │
│                                                                             │
│  async function getAndroidSharedBufferAsync(name) {                         │
│      const response = await fetch(`memio://buffer/${name}`);                │
│                                                                             │
│      // Read metadata from HTTP headers (fast!)                             │
│      const version = parseInt(response.headers.get('X-Memio-Version'));     │
│      const length = parseInt(response.headers.get('X-Memio-Length'));       │
│                                                                             │
│      // Get raw data as ArrayBuffer                                         │
│      const data = await response.arrayBuffer();                             │
│                                                                             │
│      return { data, version, length };                                      │
│  }                                                                          │
│                                                                             │
│  async function readSharedStateAndroidAsync(name) {                         │
│      const { data, version } = await getAndroidSharedBufferAsync(name);     │
│      const decoded = decode(new Uint8Array(data));  // msgpack decode       │
│      return { data: decoded, version };                                     │
│  }                                                                          │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```
---

## WRITE: Frontend → Backend (JavaScript → Rust)

### Data Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                            JAVASCRIPT FRONTEND                              │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  // User selects file or creates Blob                                       │
│  const blob = new Blob([msgpackData], { type: 'application/octet-stream' });│
│  const file = new File([blob], 'state.bin');                                │
│                                                                             │
│  // Create hidden file input and trigger native picker                      │
│  const input = document.createElement('input');                             │
│  input.type = 'file';                                                       │
│  input.click();  // Opens Android file picker                               │
│         │                                                                   │
│         │ User selects file (or programmatic via DataTransfer)              │
│         ▼                                                                   │
│  // Invoke unified Tauri command with buffer name and file URI              │
│  await invoke('plugin:memio|memio_upload', {                                │
│      bufferName: 'state',                                                   │
│      fileUri: 'content://...'                                               │
│  });                                                                        │
│                                                                             │
│  // Note: fileUri can be captured by MemioWebChromeClient                   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
             │
             │ Tauri IPC (JSON command)
             ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                              RUST MIDDLEWARE                                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  // tauri-plugin-memio/src/commands.rs                                      │
│                                                                             │
│  #[command]                                                                 │
│  pub async fn memio_upload(                                                 │
│      app: AppHandle,                                                        │
│      bufferName: String,                                                    │
│      fileUri: String                                                        │
│  ) -> Result<UploadResult, String> {                                        │
│      // Delegate to Android plugin bridge                                   │
│      android_plugin.upload_file_from_uri(bufferName, fileUri)               │
│  }                                                                          │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
             │
             │ Tauri Mobile Plugin Bridge
             ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                              KOTLIN LAYER                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  // MemioPlugin.kt                                                          │
│                                                                             │
│  @Command                                                                   │
│  fun uploadFileFromUri(invoke: Invoke) {                                    │
│      val args = invoke.parseArgs(UploadArgs::class.java)                    │
│                                                                             │
│      // Get fileUri from args OR from last file picker selection            │
│      val fileUri = args.fileUri                                             │
│          ?: MemioWebChromeClient.getLastSelectedUri()?.toString()           │
│          ?: return invoke.reject("No file URI")                             │
│                                                                             │
│      val uri = Uri.parse(fileUri)                                           │
│      val context = activity.applicationContext                              │
│                                                                             │
│      // Read file via ContentResolver (handles content:// URIs)             │
│      val inputStream = context.contentResolver.openInputStream(uri)         │
│      val bytes = inputStream?.readBytes()                                   │
│          ?: return invoke.reject("Failed to read")                          │
│                                                                             │
│      // Write directly to shared memory                                     │
│      val version = System.currentTimeMillis()                               │
│      val success = MemioSharedMemory.write(                                 │
│          args.bufferName,                                                   │
│          version,                                                           │
│          bytes                                                              │
│      )                                                                      │
│                                                                             │
│      invoke.resolve(JSObject().apply {                                      │
│          put("success", success)                                            │
│          put("bytesWritten", bytes.size)                                    │
│      })                                                                     │
│  }                                                                          │
│                                                                             │
│  // MemioSharedMemory.kt                                                    │
│                                                                             │
│  fun write(name: String, version: Long, data: ByteArray): Boolean {         │
│      return nativeWrite(name, version, data)  // JNI call                   │
│  }                                                                          │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
             │
             │ JNI: nativeWrite(name, version, data)
             ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                              RUST BACKEND                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  // memio/src/android_jni.rs                                                │
│                                                                             │
│  #[no_mangle]                                                               │
│  pub extern "system" fn Java_com_memio_shared_MemioSharedMemory_nativeWrite(│
│      mut env: JNIEnv,                                                       │
│      _class: JObject,                                                       │
│      name: JString,                                                         │
│      version: jlong,                                                        │
│      data: JByteArray,                                                      │
│  ) -> bool {                                                                │
│      let data_vec = env.convert_byte_array(&data)?;                         │
│      write_to_shared(&name_str, version as u64, &data_vec)                  │
│  }                                                                          │
│                                                                             │
│  // memio-platform/src/android.rs                                           │
│                                                                             │
│  pub fn write_to_shared(name: &str, version: u64, data: &[u8]) {            │
│      let region = REGISTRY.get_mut(name)?;                                  │
│                                                                             │
│      // Write data to mmap                                                  │
│      ptr::copy_nonoverlapping(data.as_ptr(), region.ptr + 16, data.len());  │
│                                                                             │
│      // Write header (version + length)                                     │
│      write_header_ptr(region.ptr, version, data.len());                     │
│  }                                                                          │
│                                                                             │
│  ┌─────────────────────────────────────────┐                                │
│  │     ASharedMemory (Android NDK)         │                                │
│  │  ┌───────────┬──────────────────────┐   │                                │
│  │  │  HEADER   │        DATA          │   │                                │
│  │  │ version=42│    (new bytes)       │   │                                │
│  │  │ length=N  │                      │   │                                │
│  │  └───────────┴──────────────────────┘   │                                │
│  └─────────────────────────────────────────┘                                │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```
---

## Implementation Files

### Kotlin (Android)

| File | Responsibility |
|------|----------------|
| `MemioPlugin.kt` | Android plugin command `uploadFileFromUri` |
| `MemioWebViewClient.kt` | Intercepts `memio://` and serves data via HTTP |
| `MemioWebChromeClient.kt` | Captures file picker URIs |
| `MemioJsBridge.kt` | JS interface for version polling |
| `MemioSharedMemory.kt` | JNI wrapper for shared memory |
| `MemioSpec.kt` | Constants (header size, etc) |

### Rust

| File | Responsibility |
|------|----------------|
| `memio/src/android_jni.rs` | JNI exports |
| `memio-platform/src/android.rs` | ASharedMemory implementation |
| `tauri-plugin-memio/src/commands.rs` | Unified `memio_upload` command |
| `tauri-plugin-memio/src/android.rs` | Android plugin bridge (run_mobile_plugin) |

### TypeScript

| File | Responsibility |
|------|----------------|
| `memio-client/src/android.ts` | `readSharedStateAndroidAsync()` via memio:// |
| `memio-client/src/client.ts` | Unified client API |

---

## Component Diagram

```
┌──────────────────────────────────────────────────────────────────┐
│                         FRONTEND (WebView)                       │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │                     memio-client                           │  │
│  │  ┌─────────────────┐        ┌─────────────────────────┐    │  │
│  │  │  READ           │        │  WRITE                  │    │  │
│  │  │  fetch(memio://)│        │  invoke(memio_upload)   │    │  │
│  │  └────────┬────────┘        └───────────┬─────────────┘    │  │
│  └───────────┼─────────────────────────────┼──────────────────┘  │
└──────────────┼─────────────────────────────┼─────────────────────┘
               │                             │
               ▼                             ▼
┌──────────────────────────────────────────────────────────────────┐
│                         KOTLIN LAYER                             │
│  ┌─────────────────────┐        ┌─────────────────────────────┐  │
│  │ MemioWebViewClient  │        │      MemioPlugin            │  │
│  │ shouldInterceptReq()│        │   uploadFileFromUri()       │  │
│  └─────────┬───────────┘        └──────────────┬──────────────┘  │
│            │                                   │                 │
│            ▼                                   ▼                 │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │                    MemioSharedMemory                        │ │
│  │     getDirectBuffer()              write()                  │ │
│  │           │                           │                     │ │
│  └───────────┼───────────────────────────┼─────────────────────┘ │
└──────────────┼───────────────────────────┼───────────────────────┘
               │ JNI                       │ JNI
               ▼                           ▼
┌──────────────────────────────────────────────────────────────────┐
│                         RUST LAYER                               │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │                     android_jni.rs                          │ │
│  │  nativeGetDirectBuffer()        nativeWrite()               │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                              │                                   │
│                              ▼                                   │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │                memio-platform/android.rs                    │ │
│  │                                                             │
│  │    REGISTRY: HashMap<String, AndroidSharedMemoryRegion>     │ │
│  │                                                             │
│  │    ┌─────────────────────────────────────────────────────┐  │ │
│  │    │              ASharedMemory (NDK)                    │  │ │
│  │    │  ┌────────┬─────────────────────────────────────┐   │  │ │
│  │    │  │ HEADER │              DATA                   │   │  │ │
│  │    │  │16 bytes│         (mmap region)               │   │  │ │
│  │    │  └────────┴─────────────────────────────────────┘   │  │ │
│  │    └─────────────────────────────────────────────────────┘  │ │
│  └─────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────┘
```

---

## Header Layout

```
Offset  Size   Field      Description
──────  ─────  ─────────  ──────────────────────────────
0       8      length     Data length in bytes (u64 LE)
8       8      version    Version number (u64 LE)
16      N      data       Actual payload data
```

**Note**: In Kotlin we read the header in the reverse order (offset 8 = version, offset 0 = length) because Rust writes `[length:8][version:8]`.
