# Building and Running MemioTauri

Instructions for building MemioTauri from source and running the example application on Linux.

---

## Prerequisites

MemioTauri requires:
- **Rust 1.70+** - Install via [rustup](https://rustup.rs/)
- **Node.js 18+** - For the TypeScript client
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

## Project Structure (Linux)

```
memioTauri/
├── crates/
│   ├── memio/                   # Main crate
│   ├── memio-core/              # Core types, header format
│   ├── memio-platform/          # Linux /dev/shm + mmap
│   └── tauri-plugin-memio/      # Tauri plugin
├── extensions/webkit-linux/     # WebKit extension
├── packages/memio-client/       # TypeScript client
└── examples/memio-tauri-example # Example app
```
