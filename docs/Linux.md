# Memio Linux Architecture

This document describes the communication architecture between the Rust backend and the JavaScript frontend on Linux.

## Overview

The Linux implementation uses:
- **READ (Back→Front)**: WebKit Extension with direct mmap to `/dev/shm`
- **WRITE (Front→Back)**: WebKit Extension with mmap and direct writes

Unlike Android, Linux **does not use an HTTP protocol** - the WebKit extension accesses shared memory files directly.

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
│  │  LinuxSharedMemoryFactory.create()      │                                │
│  │                                         │                                │
│  │  1. Creates file in /dev/shm            │                                │
│  │     memio_<name>_<pid>_<nonce>_<seq>.bin│                                │
│  │                                         │                                │
│  │  2. mmap with PROT_READ | PROT_WRITE    │                                │
│  │                                         │                                │
│  │  3. Write to LinuxSharedMemoryRegion:   │                                │
│  │     - write_header_unchecked()          │                                │
│  │     - copy data after header            │                                │
│  │     - mmap.flush()                      │                                │
│  └─────────────────────────────────────────┘                                │
│                     │                                                       │
│                     ▼                                                       │
│  ┌─────────────────────────────────────────┐                                │
│  │  /dev/shm/memio_state_12345_0_0.bin     │                                │
│  │  ┌───────────┬──────────────────────┐   │                                │
│  │  │  HEADER   │        DATA          │   │                                │
│  │  │ 24 bytes  │    (raw bytes)       │   │                                │
│  │  │ magic+    │                      │   │                                │
│  │  │ version+  │                      │   │                                │
│  │  │ length    │                      │   │                                │
│  │  └───────────┴──────────────────────┘   │                                │
│  └─────────────────────────────────────────┘                                │
│                                                                             │
│  SharedRegistry writes manifest:                                            │
│  /dev/shm/memio_shared_registry_<pid>.txt                                   │
│  ┌─────────────────────────────────────────┐                                │
│  │  state=/dev/shm/memio_state_12345_0.bin │                                │
│  │  config=/dev/shm/memio_config_12345.bin │                                │
│  └─────────────────────────────────────────┘                                │
│                                                                             │
│  Environment variables set:                                                 │
│  - MEMIO_SHARED_REGISTRY=/dev/shm/memio_shared_registry_<pid>.txt           │
│  - WEBKIT_WEB_EXTENSION_DIRECTORY=extensions/webkit-linux/build             │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
             │
             │ File system (/dev/shm is RAM-backed tmpfs)
             ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         WEBKIT EXTENSION (C)                                │
│                      memio_web_extension.so                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  webkit_web_extension_initialize()                                          │
│         │                                                                   │
│         │ Signal: page-created                                              │
│         ▼                                                                   │
│  on_window_object_cleared()                                                 │
│         │                                                                   │
│         ▼                                                                   │
│  load_registry(context)                                                     │
│         │                                                                   │
│         │ 1. Read MEMIO_SHARED_REGISTRY env var                             │
│         │ 2. Parse registry file (name=path lines)                          │
│         │ 3. For each buffer:                                               │
│         ▼                                                                   │
│  update_buffer(context, name, path)                                         │
│         │                                                                   │
│         │ 1. g_mapped_file_new(path) → GMappedFile                          │
│         │ 2. g_mapped_file_get_contents() → raw pointer                     │
│         │ 3. Read header: magic, version, length                            │
│         │ 4. jsc_value_new_typed_array(UINT8, total)                        │
│         │ 5. memcpy(typed_array, file_contents, total)                      │
│         │ 6. jsc_value_object_set_property(                                 │
│         │        __memioSharedBuffers, name, typed)                         │
│         ▼                                                                   │
│  Inject JS helpers:                                                         │
│    - memioSharedBuffer(name) → Uint8Array                                   │
│    - memioListBuffers() → string[]                                          │
│                                                                             │
│  Start refresh timer (100ms interval):                                      │
│    refresh_shared_buffers() → re-reads files, updates typed arrays          │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
             │
             │ globalThis.__memioSharedBuffers[name] = Uint8Array
             ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                            JAVASCRIPT FRONTEND                              │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  // memio-client/src/platform/linux.ts                                      │
│                                                                             │
│  function getLinuxSharedBuffer(name?: string): Uint8Array | null {          │
│      const bufferName = name || 'state';                                    │
│                                                                             │
│      // Try helper function first                                           │
│      if (typeof globalThis.memioSharedBuffer === 'function') {              │
│          return globalThis.memioSharedBuffer(bufferName);                   │
│      }                                                                      │
│                                                                             │
│      // Direct access to buffer map                                         │
│      if (globalThis.__memioSharedBuffers) {                                 │
│          return globalThis.__memioSharedBuffers[bufferName];                │
│      }                                                                      │
│                                                                             │
│      return null;                                                           │
│  }                                                                          │
│                                                                             │
│  // memio-client/src/shared-state.ts                                        │
│                                                                             │
│  function readSharedState(buffer: Uint8Array, lastVersion?: bigint) {       │
│      // Header layout: [magic:8][version:8][length:8][data...]              │
│      const view = new DataView(buffer.buffer);                              │
│      const magic = view.getBigUint64(0, true);                              │
│      const version = view.getBigUint64(8, true);                            │
│      const length = view.getBigUint64(16, true);                            │
│                                                                             │
│      if (lastVersion && version === lastVersion) {                          │
│          return null;  // No change                                         │
│      }                                                                      │
│                                                                             │
│      const data = buffer.subarray(24, 24 + Number(length));                 │
│      return { version, length, data };                                      │
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
│  // memio-client/src/shared-state.ts                                        │
│                                                                             │
│  function writeSharedStateBuffer(                                           │
│      buffer: Uint8Array,                                                    │
│      data: Uint8Array,                                                      │
│      version?: bigint                                                       │
│  ) {                                                                        │
│      // Direct write via WebKit extension                                   │
│      if (typeof globalThis.memioWriteSharedBuffer === 'function') {         │
│          const result = globalThis.memioWriteSharedBuffer('state', data);   │
│          return result;                                                     │
│      }                                                                      │
│      return null;                                                           │
│  }                                                                          │
│                                                                             │
│  // User code                                                               │
│  const data = encode({ counter: 42, items: [...] });  // msgpack            │
│  memioClient.writeSharedState(data);                                        │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
             │
             │ globalThis.memioWriteSharedBuffer(name, Uint8Array)
             ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         WEBKIT EXTENSION (C)                                │
│                      memio_web_extension.so                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  js_write_shared_buffer(args)                                               │
│         │                                                                   │
│         │ 1. Extract name and typed array from JS args                      │
│         │ 2. jsc_value_typed_array_get_data() → raw pointer                 │
│         ▼                                                                   │
│  Find buffer path from registry:                                            │
│         │                                                                   │
│         │ 1. Read registry file                                             │
│         │ 2. Parse "name=path" lines                                        │
│         │ 3. Find matching buffer path                                      │
│         ▼                                                                   │
│  Open and mmap file:                                                        │
│         │                                                                   │
│         │ fd = open(buffer_path, O_RDWR)                                    │
│         │ fstat(fd, &st)                                                    │
│         │ file_data = mmap(NULL, file_len,                                  │
│         │                  PROT_READ | PROT_WRITE,                          │
│         │                  MAP_SHARED, fd, 0)                               │
│         ▼                                                                   │
│  Write data + update header:                                                │
│         │                                                                   │
│         │ // Read current version                                           │
│         │ memcpy(&current_version, file_data + 8, 8);                       │
│         │                                                                   │
│         │ // Write data after header                                        │
│         │ memcpy(file_data + 24, data, data_len);                           │
│         │                                                                   │
│         │ // Update header (version + length)                               │
│         │ new_version = current_version + 1;                                │
│         │ memcpy(file_data + 8, &new_version, 8);                           │
│         │ memcpy(file_data + 16, &data_len, 8);                             │
│         │                                                                   │
│         │ munmap(file_data, file_len);                                      │
│         │ close(fd);                                                        │
│         ▼                                                                   │
│  return jsc_value_new_boolean(TRUE)                                         │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
             │
             │ File system write (RAM-backed /dev/shm)
             ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                              RUST BACKEND                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────┐                                │
│  │  /dev/shm/memio_state_12345_0_0.bin     │                                │
│  │  ┌───────────┬──────────────────────┐   │                                │
│  │  │  HEADER   │        DATA          │   │                                │
│  │  │ version++ │    (new bytes)       │   │                                │
│  │  │ length=N  │                      │   │                                │
│  │  └───────────┴──────────────────────┘   │                                │
│  └─────────────────────────────────────────┘                                │
│                     │                                                       │
│                     │ mmap remains valid - Rust sees changes immediately    │
│                     ▼                                                       │
│  MemioManager.read("state")                                                 │
│         │                                                                   │
│         │ read_header() - sees new version                                  │
│         │ read() - gets updated data                                        │
│         ▼                                                                   │
│  Process updated state from frontend                                        │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```
---

## Implementation Files

### Rust

| File | Responsibility |
|------|----------------|
| `memio-platform/src/linux.rs` | LinuxSharedMemoryFactory, LinuxSharedMemoryRegion, mmap handling |
| `memio-platform/src/registry.rs` | SharedRegistry - manages buffer manifest |
| `src/linux.rs` | Configures WEBKIT_WEB_EXTENSION_DIRECTORY and scripts |
| `src/lib.rs` | Plugin setup, injects environment variables |

### WebKit Extension (C)

| File | Responsibility |
|------|----------------|
| `extensions/webkit-linux/extension.c` | Main extension - mmap, refresh, JS bindings |
| `extensions/webkit-linux/memio_spec.h` | Header constants (magic, offsets) |
| `extensions/webkit-linux/meson.build` | Build configuration |

### TypeScript

| File | Responsibility |
|------|----------------|
| `memio-client/src/platform/linux.ts` | `hasLinuxSharedMemory()`, `getLinuxSharedBuffer()` |
| `memio-client/src/provider.ts` | `LinuxProvider` - cross-platform abstraction |
| `memio-client/src/shared-state.ts` | `readSharedState()`, `writeSharedStateBuffer()` |

---

## Component Diagram

```
┌──────────────────────────────────────────────────────────────────┐
│                         FRONTEND (WebKitGTK WebView)             │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │                     memio-client                           │  │
│  │  ┌─────────────────┐        ┌─────────────────────────┐    │  │
│  │  │  READ           │        │  WRITE                  │    │  │
│  │  │  memioShared    │        │  memioWriteShared       │    │  │
│  │  │  Buffer(name)   │        │  Buffer(name, data)     │    │  │
│  │  └────────┬────────┘        └───────────┬─────────────┘    │  │
│  └───────────┼─────────────────────────────┼──────────────────┘  │
└──────────────┼─────────────────────────────┼─────────────────────┘
               │                             │
               │ globalThis functions        │
               ▼                             ▼
┌──────────────────────────────────────────────────────────────────┐
│                   WEBKIT EXTENSION (memio_web_extension.so)      │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │                    extension.c                              │
│  │                                                             │
│  │  ┌──────────────────────┐   ┌───────────────────────────┐   │ │
│  │  │ load_registry()      │   │ js_write_shared_buffer()  │   │ │
│  │  │ update_buffer()      │   │                           │   │ │
│  │  │ refresh_shared_      │   │ open() + fstat()          │   │ │
│  │  │ buffers() [100ms]    │   │ mmap(MAP_SHARED)          │   │ │
│  │  └──────────┬───────────┘   │ memcpy() + header update  │   │ │
│  │             │               │ munmap() + close()        │   │ │
│  │             │               └───────────────┬───────────┘   │ │
│  └─────────────┼───────────────────────────────┼───────────────┘ │
└────────────────┼───────────────────────────────┼─────────────────┘
                 │ g_mapped_file                 │ mmap(O_RDWR)
                 ▼                               ▼
┌──────────────────────────────────────────────────────────────────┐
│                         /dev/shm (tmpfs)                         │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │  memio_shared_registry_<pid>.txt                            │ │
│  │  ┌───────────────────────────────────────────────────────┐  │ │
│  │  │ state=/dev/shm/memio_state_12345_0_0.bin              │  │ │
│  │  │ config=/dev/shm/memio_config_12345_1_0.bin            │  │ │
│  │  └───────────────────────────────────────────────────────┘  │ │
│  │                                                             │ │
│  │  memio_state_12345_0_0.bin                                  │ │
│  │  ┌────────┬────────┬────────┬───────────────────────────┐   │ │
│  │  │ MAGIC  │VERSION │ LENGTH │          DATA             │   │ │
│  │  │ 8 bytes│8 bytes │8 bytes │      (payload)            │   │ │
│  │  └────────┴────────┴────────┴───────────────────────────┘   │ │
│  └─────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────┘
                 ▲
                 │ mmap (PROT_READ | PROT_WRITE, MAP_SHARED)
                 │
┌──────────────────────────────────────────────────────────────────┐
│                         RUST BACKEND                             │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │                    MemioManager                             │
│  │                                                             │
│  │  create_buffer() ──► LinuxSharedMemoryFactory.create()      │
│  │  write() ──────────► region.write(version, data)            │
│  │  read() ───────────► region.read()                          │
│  │  version() ────────► region.info().version                  │
│  │                                                             │
│  └─────────────────────────────────────────────────────────────┘ │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │                 LinuxSharedMemoryRegion                     │
│  │                                                             │
│  │    MmapMut ←──── memmap2::MmapMut::map_mut(file)            │
│  │       │                                                     │
│  │       ├── write(): copy_from_slice + write_header + flush() │
│  │       ├── read(): read_header + copy to Vec                 │
│  │       └── info(): read_header only                          │
│  │                                                             │
│  └─────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────┘
```

---

## Header Layout (Linux)

```
Offset  Size   Field      Description
──────  ─────  ─────────  ──────────────────────────────
0       8      magic      Magic number: 0x4F425255545F4F54 ("MEMIO_OB")
8       8      version    Version number (u64 LE)
16      8      length     Data length in bytes (u64 LE)
24      N      data       Actual payload data
```

**Note**: The Linux header is 24 bytes (with magic), unlike Android which uses 16 bytes (length + version only).
