/**
 * MemioTauri JavaScript Helpers - Reference Implementation
 *
 * This file serves as the canonical reference for JavaScript code injected
 * into WebViews on Linux.
 *
 * IMPORTANT: Keep implementations synchronized with:
 * - extensions/webkit-linux/extension.c (Linux WebKit)
 */

// ============================================================================
// CORE HELPERS - Used by Linux WebKit
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
  return globalThis.__memioSharedBuffers ? Object.keys(globalThis.__memioSharedBuffers) : [];
};

/**
 * Debug helper to check shared memory state.
 * Returns object with availability status and buffer names.
 */
globalThis.__memioSharedDebug = function() {
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
  const buffer = globalThis.memioSharedBuffer(name);
  if (!buffer) {
    return -1;
  }
  const bytes = buffer instanceof Uint8Array ? buffer : new Uint8Array(buffer);
  if (bytes.byteLength < 16) {
    return -1;
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  return Number(view.getBigUint64(8, true));
};

// ============================================================================
// BUFFER METADATA - No data transfer
// ============================================================================

/**
 * Get buffer metadata without reading data.
 * Returns { name, version, length, capacity } or null.
 */
window.memioGetBufferInfo = function(name) {
  const buffer = globalThis.memioSharedBuffer(name);
  if (!buffer) return null;
  const bytes = buffer instanceof Uint8Array ? buffer : new Uint8Array(buffer);
  if (bytes.byteLength < 24) {
    return null;
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const version = Number(view.getBigUint64(8, true));
  const length = Number(view.getBigUint64(16, true));
  const capacity = bytes.byteLength - 64;
  return { name, version, length, capacity };
};

/**
 * Check if buffer exists.
 * Returns boolean.
 */
window.memioHasBuffer = function(name) {
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
