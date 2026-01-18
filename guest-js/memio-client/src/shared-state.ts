import { StateView } from './state-view';
import {
  SHARED_STATE_MAGIC,
  SHARED_STATE_HEADER_SIZE,
  SHARED_STATE_LENGTH_OFFSET,
  SHARED_STATE_MAGIC_OFFSET,
  SHARED_STATE_VERSION_OFFSET,
} from './shared-state-spec';
import { SHARED_MANIFEST_VERSION } from './shared-manifest-spec';
import { getAndroidSharedBuffer, hasAndroidBridge, readSharedStateAndroid } from './platform/android';
import { readSharedStateAndroidMemioProtocol } from './platform/android-memio-protocol';
import { getLinuxSharedBuffer, hasLinuxSharedMemory } from './platform/linux';
import { hasWindowsSharedBuffer, uploadViaSharedBuffer, hasSharedBufferUpload, hasSharedBufferDownload, downloadViaSharedBuffer } from './platform/windows';
import type {
  SharedStateManifest,
  SharedStateSnapshot,
  SharedStateWriteResult,
  MemioPlatform,
  MemioGlobalBase,
} from './shared-types';

/**
 * Helper to convert Uint8Array to Base64 for JavascriptInterface
 */
function uint8ArrayToBase64(bytes: Uint8Array): string {
  let binary = '';
  const chunkSize = 0x8000;
  for (let i = 0; i < bytes.length; i += chunkSize) {
    const chunk = bytes.subarray(i, i + chunkSize);
    binary += String.fromCharCode(...chunk);
  }
  return btoa(binary);
}

/**
 * Detects the current platform based on available globals.
 */
export function detectPlatform(): MemioPlatform {
  const global = globalThis as unknown as MemioGlobalBase;

  // Check Linux first - WebKitGTK extension provides __memioSharedBuffers
  // This must come before Android check since both can have __memioSharedManifest
  if (hasLinuxSharedMemory()) {
    // Double-check we're not on Android by checking user agent
    const ua = typeof navigator !== 'undefined' ? navigator.userAgent : '';
    if (!/Android/.test(ua)) {
      return 'linux';
    }
  }

  if (hasAndroidBridge()) {
    return 'android';
  }
  
  // Also check Android via user agent as fallback
  if (typeof navigator !== 'undefined' && /Android/.test(navigator.userAgent)) {
    return 'android';
  }
  
  // iOS/macOS check (WKWebView)
  if (global.webkit?.messageHandlers?.memio) {
    const ua = typeof navigator !== 'undefined' ? navigator.userAgent : '';
    if (/iPhone|iPad|iPod/.test(ua)) {
      return 'ios';
    }
    if (/Macintosh/.test(ua)) {
      return 'macos';
    }
  }

  const ua = typeof navigator !== 'undefined' ? navigator.userAgent : '';
  
  // Windows check - WebView2 or Windows user agent
  if (typeof window !== 'undefined' && 'chrome' in window && (window as any).chrome?.webview !== undefined) {
    return 'windows';
  }
  if (/Windows/.test(ua)) {
    return 'windows';
  }
  
  if (/Linux/.test(ua) && !/Android/.test(ua)) {
    return 'linux';
  }
  
  return 'unknown';
}

/**
 * Checks if memio region is available on the current platform.
 */
export function isSharedMemoryAvailable(): boolean {
  return hasLinuxSharedMemory() || hasAndroidBridge() || hasWindowsSharedBuffer();
}

export function getSharedManifest(): SharedStateManifest | null {
  const global = globalThis as unknown as MemioGlobalBase;
  const manifest = global.__memioSharedManifest;
  if (!manifest || typeof manifest !== 'object') {
    return null;
  }
  if (
    typeof manifest.version !== 'number' ||
    manifest.version !== SHARED_MANIFEST_VERSION ||
    !manifest.buffers ||
    typeof manifest.buffers !== 'object'
  ) {
    return null;
  }
  return manifest;
}

export function getMemioSharedBuffer(name?: string): ArrayBuffer | Uint8Array | null {
  return getLinuxSharedBuffer(name) ?? getAndroidSharedBuffer(name);
}

/**
 * Reads data from memio buffer (direct on Windows).
 * - Linux: Uses WebKit extension globals
 * - Android: Uses memio:// protocol ONLY (ZERO BASE64! NO FALLBACK!)
 * - Windows: Uses WebView2 SharedBuffer API (direct!)
 * 
 * @param name - Buffer name
 * @returns Promise with { version, data } or null if not available
 */
export async function readMemioSharedBuffer(
  name: string
): Promise<{ version: bigint; data: Uint8Array } | null> {
  // Android: ONLY memio:// protocol - NO FALLBACK!
  if (hasAndroidBridge()) {
    console.log(`[MemioClient] Android: using memio:// protocol ONLY (NO FALLBACK)`);
    const snapshot = await readSharedStateAndroidMemioProtocol(name);
    if (!snapshot) {
      console.error(`[MemioClient] Android: memio:// protocol FAILED - no fallback!`);
      return null;
    }
    // StateView provides direct access to bytes via its internal uint8View
    const bytes = new Uint8Array(snapshot.view.rawBuffer);
    console.log(`[MemioClient] Android: read ${bytes.length} bytes from '${name}' (memio:// ZERO BASE64!)`);
    return { version: snapshot.version, data: bytes };
  }
  
  // Windows: Use SharedBuffer for direct download
  if (hasWindowsSharedBuffer() && hasSharedBufferDownload()) {
    console.log(`[MemioClient] Windows: attempting SharedBuffer ZERO-COPY download...`);
    const result = await downloadViaSharedBuffer(name);
    if (result) {
      console.log(`[MemioClient] Windows: read ${result.data.length} bytes from '${name}' (SharedBuffer ZERO-COPY)`);
      return result;
    }
    console.warn(`[MemioClient] Windows: SharedBuffer download failed, buffer may not exist`);
    return null;
  }
  
  // Linux: Use synchronous buffer access - extract data from header
  const buffer = getMemioSharedBuffer(name);
  if (buffer) {
    const bytes = buffer instanceof Uint8Array ? buffer : new Uint8Array(buffer);
    
    // Buffer includes header: [magic:8][version:8][length:8][...reserved...][data]
    // Header size is SHARED_STATE_HEADER_SIZE (64 bytes)
    if (bytes.length < SHARED_STATE_HEADER_SIZE) {
      console.warn(`[MemioClient] Linux: buffer too small (${bytes.length} < ${SHARED_STATE_HEADER_SIZE})`);
      return null;
    }
    
    // Read version and length from header
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const version = view.getBigUint64(SHARED_STATE_VERSION_OFFSET, true);
    const length = Number(view.getBigUint64(SHARED_STATE_LENGTH_OFFSET, true));
    
    // Extract only the data portion (after header)
    const data = bytes.subarray(SHARED_STATE_HEADER_SIZE, SHARED_STATE_HEADER_SIZE + length);
    
    console.log(`[MemioClient] Linux: read ${data.length} bytes from '${name}' (version ${version})`);
    return { version, data };
  }
  
  return null;
}

/**
 * Writes data directly to memio buffer.
 * - Linux: Uses WebKit extension (memioWriteSharedBuffer)
 * - Android: Uses MemioNative bridge
 * - Windows: Uses Tauri IPC (plugin:memio|write_shared_buffer_windows)
 * 
 * Waits for buffer to exist before writing (backend must create it first).
 * @param name - Buffer name
 * @param data - Data to write (Uint8Array or ArrayBuffer)
 * @param timeoutMs - Maximum time to wait for buffer (default: 5000ms)
 * @returns Promise that resolves to true if successful, false otherwise
 */
export async function writeMemioSharedBuffer(
  name: string,
  data: Uint8Array | ArrayBuffer,
  timeoutMs = 5000
): Promise<boolean> {
  const global = globalThis as any;
  const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
  
  // Android: Use JavascriptInterface for writes (memio:// POST not supported - no body access in WebResourceRequest)
  if (hasAndroidBridge()) {
    console.log(`[MemioClient] Android: using JavascriptInterface for write (memio:// POST not supported)`);
    const startTime = Date.now();
    let attempts = 0;
    
    while (Date.now() - startTime < timeoutMs) {
      attempts++;
      
      try {
        // Convert to Base64 for JavascriptInterface
        const base64 = uint8ArrayToBase64(bytes);
        const version = Date.now();
        
        // Use JavascriptInterface write
        const bridge = global.__TAURI_MEMIO__ ?? global.MemioNative;
        if (!bridge || typeof bridge.write !== 'function') {
          throw new Error('Memio Android bridge not available');
        }
        bridge.write(name, version, base64);
        console.log(`[MemioClient] Android: wrote ${bytes.length} bytes to buffer '${name}' (attempt ${attempts}, JavascriptInterface)`);
        return true;
      } catch (error) {
        console.debug(`[MemioClient] Android write attempt ${attempts} failed, retrying...`, error);
      }
      
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    
    console.error(`[MemioClient] Android: failed to write to buffer '${name}' after ${attempts} attempts`);
    return false;
  }
  
  // Windows: ONLY SharedBuffer (direct) - NO FALLBACKS for testing
  if (hasWindowsSharedBuffer()) {
    const version = Number(BigInt(Date.now()));
    
    // ONLY SharedBuffer - no fallbacks!
    if (hasSharedBufferUpload()) {
      console.log(`[MemioClient] Windows: attempting SharedBuffer ZERO-COPY upload...`);
      const success = await uploadViaSharedBuffer(name, bytes, version);
      if (success) {
        console.log(`[MemioClient] Windows: wrote ${bytes.length} bytes to buffer '${name}' (SharedBuffer ZERO-COPY)`);
        return true;
      } else {
        console.error(`[MemioClient] Windows: SharedBuffer upload FAILED!`);
        return false;
      }
    } else {
      console.error(`[MemioClient] Windows: hasSharedBufferUpload() returned false!`);
      return false;
    }
  }
  
  // Linux: use memioWriteSharedBuffer from WebKit extension
  if (typeof global.memioWriteSharedBuffer !== 'function') {
    console.warn('[MemioClient] memioWriteSharedBuffer not available - Linux WebKit extension, Android bridge, or Windows Tauri required');
    return false;
  }

  // Wait for buffer to be created by backend (with retry)
  const startTime = Date.now();
  let attempts = 0;
  
  while (Date.now() - startTime < timeoutMs) {
    attempts++;
    
    try {
      const result = global.memioWriteSharedBuffer(name, bytes);
      if (result === true) {
        console.log(`[MemioClient] Successfully wrote ${bytes.length} bytes to buffer '${name}' (attempt ${attempts})`);
        return true;
      }
    } catch (error) {
      // Buffer might not exist yet, retry
      console.debug(`[MemioClient] Write attempt ${attempts} failed, retrying...`, error);
    }
    
    // Wait a bit before retrying
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  
  console.error(`[MemioClient] Failed to write to buffer '${name}' after ${attempts} attempts (${timeoutMs}ms). Backend must create buffer first with manager.create_buffer("${name}", size)`);
  return false;
}

/**
 * Waits for the memio buffer to become available.
 * @param name - Buffer name (default: 'state')
 * @param timeoutMs - Maximum time to wait in milliseconds (default: 2000)
 * @returns Promise that resolves to the buffer or null if timeout
 */
export async function waitForSharedBuffer(
  name?: string,
  timeoutMs = 2000
): Promise<ArrayBuffer | Uint8Array | null> {
  const startTime = Date.now();
  const bufferName = name ?? 'state';
  
  console.debug('[Memio] waitForSharedBuffer called for:', bufferName, 'timeout:', timeoutMs);
  
  while (Date.now() - startTime < timeoutMs) {
    // Check if any bridge is available
    const hasLinux = hasLinuxSharedMemory();
    const hasAndroid = hasAndroidBridge();
    const hasWindows = hasWindowsSharedBuffer();
    
    console.debug('[Memio] Platforms - Linux:', hasLinux, 'Android:', hasAndroid, 'Windows:', hasWindows);
    
    // For Android, just check if MemioNative exists - we use memio:// protocol for reads
    // which is async, so we can't check buffer contents synchronously
    if (hasAndroid) {
      console.debug('[MemioAndroid] MemioNative available, returning success marker');
      return new Uint8Array(0);
    }
    
    if (hasLinux) {
      // Extension loaded: expose success marker even if buffer isn't ready yet.
      if (typeof (globalThis as any).memioSharedBuffer === 'function') {
        return new Uint8Array(0);
      }
      const buffer = getMemioSharedBuffer(bufferName);
      if (buffer) {
        return buffer;
      }
    }
    
    // For Windows with Tauri, SharedBuffer is always available (on-demand creation)
    // No need to wait for a specific buffer - just return success marker
    if (hasWindows) {
      console.debug('[MemioWindows] SharedBuffer API available, returning success marker');
      return new Uint8Array(0);
    }
    
    await new Promise(resolve => setTimeout(resolve, 50));
  }
  
  console.debug('[Memio] waitForSharedBuffer timeout');
  return null;
}

export function readSharedState(
  buffer: ArrayBuffer | Uint8Array,
  lastVersion?: bigint
): SharedStateSnapshot | null {
  const raw = buffer instanceof Uint8Array ? buffer : new Uint8Array(buffer);
  const header = parseSharedStateHeader(raw, lastVersion);
  if (!header) {
    return null;
  }
  const dataStart = SHARED_STATE_HEADER_SIZE;
  const bytes = raw.subarray(dataStart, dataStart + header.length);
  return {
    version: header.version,
    length: header.length,
    view: new StateView(bytes),
  };
}

export function writeSharedStateBuffer(
  buffer: ArrayBuffer | Uint8Array,
  data: ArrayBuffer | Uint8Array,
  version?: bigint
): SharedStateWriteResult | null {
  const raw = buffer instanceof Uint8Array ? buffer : new Uint8Array(buffer);
  if (raw.byteLength < SHARED_STATE_HEADER_SIZE) {
    return null;
  }

  const dataBytes = data instanceof Uint8Array ? data : new Uint8Array(data);
  const capacity = raw.byteLength - SHARED_STATE_HEADER_SIZE;
  if (dataBytes.byteLength > capacity) {
    return null;
  }

  const view = new DataView(raw.buffer, raw.byteOffset, raw.byteLength);
  let nextVersion = version;
  if (nextVersion === undefined) {
    const currentMagic = view.getBigUint64(SHARED_STATE_MAGIC_OFFSET, true);
    const currentVersion =
      currentMagic === SHARED_STATE_MAGIC
        ? view.getBigUint64(SHARED_STATE_VERSION_OFFSET, true)
        : BigInt(0);
    nextVersion = currentVersion + BigInt(1);
  }

  view.setBigUint64(SHARED_STATE_MAGIC_OFFSET, SHARED_STATE_MAGIC, true);
  view.setBigUint64(SHARED_STATE_VERSION_OFFSET, nextVersion, true);
  view.setBigUint64(SHARED_STATE_LENGTH_OFFSET, BigInt(dataBytes.byteLength), true);
  raw.set(dataBytes, SHARED_STATE_HEADER_SIZE);

  return { version: nextVersion, length: dataBytes.byteLength };
}

export { readSharedStateAndroid };
export type { SharedStateManifest, SharedStateSnapshot, SharedStateWriteResult, MemioPlatform };
export {
  SHARED_STATE_HEADER_SIZE,
  SHARED_STATE_MAGIC,
  SHARED_STATE_LENGTH_OFFSET,
  SHARED_STATE_MAGIC_OFFSET,
  SHARED_STATE_VERSION_OFFSET,
  SHARED_STATE_ENDIANNESS,
} from './shared-state-spec';

function parseSharedStateHeader(
  raw: Uint8Array,
  lastVersion?: bigint
): { version: bigint; length: number } | null {
  if (raw.byteLength < SHARED_STATE_HEADER_SIZE) {
    return null;
  }

  const view = new DataView(raw.buffer, raw.byteOffset, raw.byteLength);
  const magic = view.getBigUint64(SHARED_STATE_MAGIC_OFFSET, true);
  if (magic !== SHARED_STATE_MAGIC) {
    return null;
  }

  const length = Number(view.getBigUint64(SHARED_STATE_LENGTH_OFFSET, true));
  const version = view.getBigUint64(SHARED_STATE_VERSION_OFFSET, true);
  if (!Number.isFinite(length) || length <= 0) {
    return null;
  }
  if (lastVersion !== undefined && version === lastVersion) {
    return null;
  }

  const dataStart = SHARED_STATE_HEADER_SIZE;
  if (length > raw.byteLength - dataStart) {
    return null;
  }

  return { version, length };
}
