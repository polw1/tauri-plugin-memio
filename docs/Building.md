# Building and Running MemioTauri

Instructions for building MemioTauri from source and running the example application on Linux and Android.

---

## Prerequisites

MemioTauri requires:
- **Rust 1.70+** - Install via [rustup](https://rustup.rs/)
- **Node.js 18+** - For TypeScript client
- **Tauri CLI 2.x** - `cargo install tauri-cli@^2.0`

---

## Linux Build

### Dependencies

```bash
# Debian/Ubuntu
sudo apt install \
  libwebkit2gtk-4.1-dev \
  meson \
  ninja-build \
  build-essential \
  pkg-config \
  libgtk-3-dev \
  libsoup-3.0-dev \
  libjavascriptcoregtk-4.1-dev

# Fedora
sudo dnf install \
  webkit2gtk4.1-devel \
  meson \
  ninja-build \
  gtk3-devel \
  libsoup3-devel
```

### Build Steps

```bash
# Clone repository
git clone https://github.com/pdc-labs/memioTauri.git
cd memioTauri

# Build TypeScript client
cd packages/memio-client
npm install
npm run build
cd ../..

# Build and run example
cd examples/memio-tauri-example
npm install
npm run tauri dev
```

### Linux Architecture Summary

```
┌─────────────────────────────────────────────────────────────┐
│                     JavaScript (WebView)                    │
│  memioSharedBuffer('state') → Uint8Array                    │
│  memioWriteSharedBuffer('state', data) → boolean            │
└─────────────────────────────┬───────────────────────────────┘
                              │ globalThis functions
                              ▼
┌─────────────────────────────────────────────────────────────┐
│              WebKit Extension (memio_web_extension.so)      │
│  g_mapped_file_new() → mmap → memcpy to TypedArray          │
└─────────────────────────────┬───────────────────────────────┘
                              │ /dev/shm files
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                     Rust Backend                            │
│  MemioManager → LinuxSharedMemoryRegion → mmap → /dev/shm   │
└─────────────────────────────────────────────────────────────┘
```

**Performance:**
- READ: ~1-2ms (zero-copy via mmap)
- WRITE: ~1ms (direct mmap write)
- Polling: WebKit extension timer (100ms)

### WebKit Extension

The extension is built automatically during `cargo tauri dev`. To rebuild manually:

```bash
cd extensions/webkit-linux/build
meson setup --wipe .
ninja
```

The plugin automatically:
1. Sets `WEBKIT_WEB_EXTENSION_DIRECTORY` environment variable
2. Sets `MEMIO_SHARED_REGISTRY` pointing to the registry file
3. Creates WebView with `extensions_path()` configured

---

## Android Build

### Dependencies

1. **Android Studio** with:
   - Android SDK API 34+
   - Android NDK 26+
   - Build Tools 34.0.0+
   - Minimum SDK version: 26 (required by `tauri-plugin-memio`)

2. **Rust Android targets**:
   ```bash
   rustup target add aarch64-linux-android armv7-linux-androideabi
   ```

3. **Environment variables** (add to `~/.bashrc` or `~/.zshrc`):
   ```bash
   export ANDROID_HOME=$HOME/Android/Sdk
   export NDK_HOME=$ANDROID_HOME/ndk/26.0.10792818
   export PATH=$PATH:$ANDROID_HOME/platform-tools
   ```

### Build Steps

```bash
# Initialize Android support (first time only)
cd examples/memio-tauri-example
cargo tauri android init

# Build and run on connected device (sets adb reverse + host)
npm run android:tauri:dev

# Or target specific device
npm run tauri android dev -- --device <device-id>

# List available devices
adb devices
```

### Android Architecture Summary

```
┌─────────────────────────────────────────────────────────────┐
│                     JavaScript (WebView)                    │
│  READ:  fetch('memio://buffer/state') → ArrayBuffer         │
│  WRITE: invoke('plugin:memio|memio_upload')                 │
└──────────────┬────────────────────────────┬─────────────────┘
               │                            │
               ▼                            ▼
┌──────────────────────────┐  ┌────────────────────────────────┐
│  MemioWebViewClient      │  │  MemioPlugin.kt                │
│  shouldInterceptRequest  │  │  uploadFileFromUri()           │
│  → memio:// protocol     │  │  → ContentResolver             │
└──────────────┬───────────┘  └──────────────┬─────────────────┘
               │                             │
               ▼                             ▼
┌─────────────────────────────────────────────────────────────┐
│              MemioSharedMemory.kt (JNI wrapper)             │
│  getDirectBuffer(name) → DirectByteBuffer                   │
│  write(name, version, data) → boolean                       │
└──────────────────────────────┬──────────────────────────────┘
                               │ JNI calls
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                     Rust Backend                            │
│  android_jni.rs → memio-platform/android.rs                 │
│  ASharedMemory (NDK) → mmap                                 │
└─────────────────────────────────────────────────────────────┘
```

### Android Plugin Setup

The Tauri plugin automatically registers these components:

| Component | Purpose |
|-----------|---------|
| `MemioPlugin.kt` | Android command `uploadFileFromUri` (used by unified `memio_upload`) |
| `MemioWebViewClient.kt` | Intercepts `memio://` URLs |
| `MemioWebChromeClient.kt` | Captures file picker URIs |
| `MemioJsBridge.kt` | JavaScript interface for version polling |
| `MemioSharedMemory.kt` | JNI wrapper for shared memory |

---

## Project Structure

```
memioTauri/
├── crates/
│   ├── memio/                   # Main crate with JNI bindings
│   │   └── src/android_jni.rs   # JNI exports for Android
│   ├── memio-core/              # Core types, header format
│   ├── memio-platform/          # Platform implementations
│   │   ├── src/linux.rs         # Linux /dev/shm + mmap
│   │   └── src/android.rs       # Android ASharedMemory
│   └── tauri-plugin-memio/      # Tauri plugin
│       ├── src/lib.rs           # Plugin setup
│       ├── src/linux.rs         # WebKit extension config
│       ├── src/android.rs       # upload_file_from_uri command
│       └── android/src/main/java/com/memio/
│           ├── MemioPlugin.kt
│           ├── MemioWebViewClient.kt
│           ├── MemioWebChromeClient.kt
│           ├── MemioJsBridge.kt
│           └── shared/MemioSharedMemory.kt
├── packages/
│   └── memio-client/            # TypeScript client
│       └── src/
│           ├── client.ts        # MemioClient API
│           ├── provider.ts      # Platform providers
│           └── platform/
│               ├── linux.ts     # Linux-specific
│               └── android.ts   # Android memio:// fetch
├── extensions/
│   └── webkit-linux/            # WebKitGTK extension (C)
│       ├── extension.c          # Main extension code
│       ├── memio_spec.h         # Header constants
│       └── meson.build          # Build config
├── docs/
│   ├── android.md               # Android architecture
│   ├── linux.md                 # Linux architecture
│   └── Building.md              # This file
└── examples/
    └── memio-tauri-example/     # Demo application
```

---

## Development Workflow

### Regenerating Shared Specs

After modifying `shared/shared_state_spec.json`:

```bash
node scripts/gen_shared_state_spec.js
```

This generates:
- `crates/memio-core/src/shared_state_spec.rs`
- `packages/memio-client/src/shared-state-spec.ts`
- `extensions/webkit-linux/memio_spec.h`
- `crates/tauri-plugin-memio/android/.../MemioSpec.kt`

### Running Tests

```bash
# Rust tests
cargo test

# TypeScript tests
cd packages/memio-client
npm test
```

### Release Builds

```bash
# Linux
cd examples/memio-tauri-example
npm run tauri build

# Android APK
npm run tauri android build
```

---

## Troubleshooting

### Linux: "Shared memory not available"

**Symptom:** `memioSharedBuffer('state')` returns null

**Causes & Solutions:**

1. **WebKit extension not loaded:**
   ```bash
   # Check extension exists
   ls extensions/webkit-linux/build/*.so
   
   # Rebuild if missing
   cd extensions/webkit-linux/build
   meson setup --wipe . && ninja
   ```

2. **Buffer not created yet:**
   ```rust
   // In Rust setup, ensure buffer is created
   let manager = MemioManager::new()?;
   manager.create_buffer("state", 1024 * 1024)?;
   ```

3. **Registry not found:**
   - Check `MEMIO_SHARED_REGISTRY` environment variable is set
   - Verify registry file exists: `ls /dev/shm/memio_shared_registry_*.txt`

### Android: "memio:// returns empty"

**Symptom:** `fetch('memio://buffer/state')` returns empty data

**Causes & Solutions:**

1. **Buffer not initialized:**
   ```rust
   // In Rust, create buffer before frontend reads
   memio_manager.create_buffer("state", 1024 * 1024)?;
   ```

2. **MemioWebViewClient not registered:**
   - Check logcat for "MemioWebViewClient" messages
   - Verify plugin is initialized in MainActivity

3. **SharedMemory not accessible:**
   ```kotlin
   // Debug: check if buffer exists
   Log.d("Memio", "exists: ${MemioSharedMemory.exists("state")}")
   Log.d("Memio", "regions: ${MemioSharedMemory.listRegions()}")
   ```

### Android: "upload_file_from_uri failed"

**Symptom:** Write from JavaScript fails

**Causes & Solutions:**

1. **No file URI captured:**
   - Ensure file picker was triggered before invoke
   - Check `MemioWebChromeClient.getLastSelectedUri()`

2. **ContentResolver permission:**
   - The file picker grants temporary URI permissions
   - URI must be used immediately, not stored

### Build Errors

| Error | Solution |
|-------|----------|
| `cannot find -lwebkit2gtk-4.1` | `sudo apt install libwebkit2gtk-4.1-dev` |
| `NDK not found` | Set `NDK_HOME` environment variable |
| `Android SDK not found` | Set `ANDROID_HOME` environment variable |
| `Kotlin version mismatch` | Update Kotlin plugin in `build.gradle.kts` |

---

## Next Steps

- Read [Linux Architecture](linux.md) for Linux implementation details
- Read [Android Architecture](android.md) for Android implementation details
- Check [Examples](../examples/memio-tauri-example) for sample code
