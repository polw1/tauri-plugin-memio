/**
 * MemioTauri JavaScript Helpers - Reference Implementation
 * 
 * This file serves as the canonical reference for JavaScript code injected
 * into WebViews across different platforms.
 * 
 * IMPORTANT: Keep implementations synchronized with:
 * - extensions/webkit-linux/extension.c (Linux WebKit)
 * - crates/tauri-plugin-memio/android/src/main/java/com/memio/jsbridge/MemioJsBridge.kt (Android)
 * 
 * ## Architecture (Android - Zero Base64)
 * 
 * Data transfer uses native protocols, not Base64:
 * - READ: memio:// protocol (MemioWebViewClient) → raw bytes via WebResourceResponse
 * - WRITE: upload_file_from_uri command → ContentResolver → MemioSharedMemory
 * 
 * This file only provides:
 * - Buffer metadata (list, exists, info, version)
 * - Debug utilities
 * - Manifest management
 */

// ============================================================================
// CORE HELPERS - Used by ALL platforms
// ============================================================================

/**
 * Get a shared buffer by name from cache.
 * Returns the cached buffer or null if not available.
 */
globalThis.memioSharedBuffer = function(name) {
  name = name || 'state';
  return globalThis.__memioSharedBuffers ? globalThis.__memioSharedBuffers[name] : null;
};

/**
 * List all available buffer names.
 * Returns an array of buffer names or empty array if none available.
 */
globalThis.memioListBuffers = function() {
  // Try native first (Android)
  if (typeof window !== 'undefined' && window.MemioNative?.listBuffers) {
    try {
      return JSON.parse(window.MemioNative.listBuffers());
    } catch (e) {
      // fall through
    }
  }
  return globalThis.__memioSharedBuffers ? Object.keys(globalThis.__memioSharedBuffers) : [];
};

/**
 * Debug helper to check shared memory state.
 * Returns object with availability status and buffer names.
 */
globalThis.__memioSharedDebug = function() {
  // Try native first (Android)
  if (typeof window !== 'undefined' && window.MemioNative?.debug) {
    try {
      return JSON.parse(window.MemioNative.debug());
    } catch (e) {
      // fall through
    }
  }
  return {
    has: !!globalThis.__memioSharedBuffers,
    keys: globalThis.__memioSharedBuffers ? Object.keys(globalThis.__memioSharedBuffers) : []
  };
};

// ============================================================================
// VERSION CHECK - Fast polling without data transfer
// ============================================================================

/**
 * Get current version of a buffer.
 * Returns version number or -1 on error.
 */
window.memioGetVersion = function(name) {
  name = name || 'state';
  if (typeof window !== 'undefined' && window.MemioNative?.getVersion) {
    try {
      return window.MemioNative.getVersion(name);
    } catch (e) {
      return -1;
    }
  }
  return -1;
};

// ============================================================================
// BUFFER METADATA - No data transfer
// ============================================================================

/**
 * Get buffer metadata without reading data.
 * Returns { name, version, length, capacity, isDirect } or null.
 */
window.memioGetBufferInfo = function(name) {
  if (typeof window !== 'undefined' && window.MemioNative?.getBufferInfo) {
    try {
      const result = window.MemioNative.getBufferInfo(name);
      if (!result) return null;
      return JSON.parse(result);
    } catch (e) {
      return null;
    }
  }
  return null;
};

/**
 * Check if buffer exists.
 * Returns boolean.
 */
window.memioHasBuffer = function(name) {
  if (typeof window !== 'undefined' && window.MemioNative?.hasBuffer) {
    try {
      return window.MemioNative.hasBuffer(name);
    } catch (e) {
      return false;
    }
  }
  return globalThis.__memioSharedBuffers ? !!globalThis.__memioSharedBuffers[name] : false;
};

// ============================================================================
// MANIFEST - Buffer discovery
// ============================================================================

/**
 * Refresh shared manifest for unified buffer discovery.
 * Returns { version: 1, buffers: { ... } }
 */
window.__memioRefreshManifest = function() {
  const names = window.memioListBuffers();
  const buffers = {};
  for (let i = 0; i < names.length; i++) {
    buffers[names[i]] = {};
  }
  window.__memioSharedManifest = { version: 1, buffers };
  return window.__memioSharedManifest;
};
