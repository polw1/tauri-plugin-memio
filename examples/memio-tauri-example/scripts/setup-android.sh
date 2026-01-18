#!/bin/bash
# MemioTauri Android Setup Script
# 
# This script configures Gradle to include the memio plugin manually,
# bypassing Tauri CLI's limitation of only detecting direct Cargo dependencies.
#
# Run this after `cargo tauri android init` or when Gradle files are regenerated.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ANDROID_DIR="$SCRIPT_DIR/../src-tauri/gen/android"
APP_DIR="$ANDROID_DIR/app"

echo "🚀 MemioTauri Android Setup"
echo "============================="

# Check if Android directory exists
if [ ! -d "$ANDROID_DIR" ]; then
    echo "❌ Android directory not found at: $ANDROID_DIR"
    echo "   Run 'cargo tauri android init' first."
    exit 1
fi

# 1. Add memio plugin to settings.gradle if not already present
SETTINGS_FILE="$ANDROID_DIR/settings.gradle"
if grep -q "tauri-plugin-memio" "$SETTINGS_FILE"; then
    echo "✅ settings.gradle already configured"
else
    echo "📝 Configuring settings.gradle..."
    cat >> "$SETTINGS_FILE" << 'EOF'

// MemioTauri plugin (manual include - bypasses Tauri CLI detection)
include ':tauri-plugin-memio'
project(':tauri-plugin-memio').projectDir = new File("../../../../../android")
EOF
    echo "✅ settings.gradle updated"
fi

# 2. Create memio.build.gradle.kts if not exists
MEMIO_GRADLE="$APP_DIR/memio.build.gradle.kts"
if [ -f "$MEMIO_GRADLE" ]; then
    echo "✅ memio.build.gradle.kts already exists"
else
    echo "📝 Creating memio.build.gradle.kts..."
    cat > "$MEMIO_GRADLE" << 'EOF'
// MemioTauri plugin dependency
// This file is applied after tauri.build.gradle.kts to add the memio plugin

val implementation by configurations

dependencies {
    implementation(project(":tauri-plugin-memio"))
}
EOF
    echo "✅ memio.build.gradle.kts created"
fi

# 3. Add apply line to build.gradle.kts if not present
BUILD_FILE="$APP_DIR/build.gradle.kts"
if grep -q 'apply(from = "memio.build.gradle.kts")' "$BUILD_FILE"; then
    echo "✅ build.gradle.kts already configured"
else
    echo "📝 Configuring build.gradle.kts..."
    # Add the apply line after tauri.build.gradle.kts
    sed -i 's|apply(from = "tauri.build.gradle.kts")|apply(from = "tauri.build.gradle.kts")\n\n// MemioTauri plugin (manual include - bypasses Tauri CLI detection)\napply(from = "memio.build.gradle.kts")|' "$BUILD_FILE"
    echo "✅ build.gradle.kts updated"
fi

echo ""
echo "✅ MemioTauri Android setup complete!"
echo ""
echo "You can now run: cargo tauri android dev"
