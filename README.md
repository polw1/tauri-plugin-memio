# MemioTauri

**High-performance data bridge for Tauri apps.** Data transfer between Rust and JavaScript.

---

## What is a memio region?

A memio region is the shared data area that Memio creates between the Rust backend and the WebView frontend.  
The implementation varies by platform (for example, `/dev/shm` on Linux, `ASharedMemory` on Android, or a Windows file mapping / SharedBuffer), but the API stays the same.

---

## Installation

See [Building and Running](docs/Building.md) for complete setup instructions.

## Supported Platforms

| Platform | Supported |
| -------- | --------- |
| Linux    | ✓         |
| Windows  | ✓         |
| Android  | ✓         |
| macOS    | x         |
| iOS      | x         |

**Quick install:**

```toml
# src-tauri/Cargo.toml
[dependencies]
memio = { path = "path/to/memioTauri/crates/memio" }
```

```json
// package.json
{
  "dependencies": {
    "memio-client": "file:path/to/memioTauri/guest-js/memio-client"
  }
}
```

---

## Usage Examples

### Backend → Frontend (Rust writes, JavaScript reads)

**Rust:**
```rust
use memio::prelude::*;

// Create memio buffer
let manager = MemioManager::new()?;
manager.create_buffer("state", 1024 * 1024)?; // 1MB

// Write data (version provided by caller)
let data = b"Hello from Rust!";
manager.write("state", 1, data)?;
```

**JavaScript:**
```typescript
import { MemioClient } from 'memio-client';

const memio = new MemioClient();

// Wait for buffer to be ready
await memio.waitForSharedMemory('state');

// Read data (async for Android, sync for desktop)
const snapshot = await memio.readSharedStateAsync();
if (snapshot) {
  console.log('Version:', snapshot.version);
  console.log('Data:', new TextDecoder().decode(snapshot.data));
}
```

---

### Frontend → Backend (JavaScript writes, Rust reads)

**JavaScript:**
```typescript
import { MemioClient } from 'memio-client';

const memio = new MemioClient();

// Write data to memio region
const data = new TextEncoder().encode('Hello from JS!');
memio.writeSharedState(data);
```

**Rust:**
```rust
use memio::prelude::*;

let manager = MemioManager::new()?;

// Read data written by frontend
let result = manager.read("state")?;
println!("Version: {}", result.version);
println!("Data: {:?}", String::from_utf8_lossy(&result.data));
```

---

## Tauri Integration (minimal client setup)

The goal is: the client only enables the Memio plugin, and Memio handles the
platform-specific details (Android vs desktop).

1) Register the plugin in your Rust app:

```rust
tauri::Builder::default()
  .plugin(memio::plugin::init())
  .invoke_handler(tauri::generate_handler![
    // your app commands...
  ])
  .run(tauri::generate_context!())?;
```

2) Allow only the Memio plugin in capabilities:

```json
// src-tauri/capabilities/default.json
[
  {
    "identifier": "desktop",
    "description": "Desktop permissions for the app",
    "windows": ["main"],
    "local": true,
    "remote": {
      "urls": ["http://tauri.localhost/*", "https://tauri.localhost/*"]
    },
    "permissions": ["memio:default"],
    "platforms": ["linux", "windows", "macOS"]
  },
  {
    "identifier": "android",
    "description": "Android permissions for the app (about:* webview bootstrap)",
    "windows": ["main"],
    "local": true,
    "remote": {
      "urls": ["about:*"]
    },
    "permissions": ["memio:default"],
    "platforms": ["android"]
  }
]
```

3) Reference the capabilities in `tauri.conf.json`:

```json
{
  "app": {
    "security": {
      "capabilities": ["desktop", "android"]
    }
  }
}
```

This keeps the client config minimal while still allowing the Android WebView
bootstrap URL (`about:blank`) to access the Memio commands.

---

## Documentation

- [Building and Running](docs/Building.md) - Setup and installation
- [Linux Architecture](docs/Linux.md) - WebKit extension + mmap
- [Android Architecture](docs/Android.md) - memio:// protocol + JNI
- [Architecture Overview](docs/Architecture.md) - Technical details

---

## License

MIT
