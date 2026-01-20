/**
 * Unified Memio API
 * 
 * Cross-platform API for memio region operations.
 * The framework handles all platform-specific complexity internally.
 * 
 * ## Usage
 * 
 * ```typescript
 * import { memioRead, memioWrite, memioUpload } from 'memio-client';
 * 
 * // Read from memio region
 * const data = await memioRead('buffer_name');
 * 
 * // Write data to memio region
 * await memioWrite('buffer_name', myData);
 * 
 * // Upload file to memio region (from file picker)
 * await memioUpload('buffer_name', fileUri);
 * ```
 */

import { invoke } from '@tauri-apps/api/core';
import { detectPlatform, writeMemioSharedBuffer } from './shared-state';
import { readSharedStateAndroidMemioProtocol } from './platform/android-memio-protocol';
import { StateView } from './state-view';

/**
 * Result of a memio read operation.
 */
export interface MemioReadResult {
  /** Raw data from memio region */
  data: Uint8Array;
  /** Version of the data */
  version: bigint;
  /** Length in bytes */
  length: number;
  /** Helper for reading typed data */
  view: StateView;
}

/**
 * Result of a memio write/upload operation.
 */
export interface MemioWriteResult {
  /** Whether the operation succeeded */
  success: boolean;
  /** Number of bytes written */
  bytesWritten: number;
  /** New version after write */
  version: bigint;
  /** Time taken in milliseconds */
  durationMs: number;
}

declare global {
  interface Window {
    memioSharedBuffer?: (name: string) => ArrayBuffer | null;
    memioWriteSharedBuffer?: (name: string, data: Uint8Array) => boolean;
  }
}

/**
 * Read data from memio buffer.
 * 
 * This is the unified read API that works on all platforms:
 * - **Linux**: Uses WebKit extension (mmap, direct)
 * - **Android**: Uses memio:// protocol
 * - **Windows**: Uses SharedBuffer API
 * 
 * @param bufferName - Name of the memio buffer
 * @param lastVersion - Optional: skip read if version hasn't changed
 * @returns MemioReadResult with data, version, and helper view
 */
export async function memioRead(
  bufferName: string = 'state',
  lastVersion?: bigint
): Promise<MemioReadResult | null> {
  const platform = detectPlatform();
  
  switch (platform) {
    case 'linux':
      return readLinux(bufferName, lastVersion);
    case 'android':
      return readAndroid(bufferName, lastVersion);
    case 'windows':
      return readWindows(bufferName, lastVersion);
    default:
      console.warn(`[Memio] Unsupported platform: ${platform}`);
      return null;
  }
}

/**
 * Write data to memio buffer.
 * 
 * This is the unified write API that works on all platforms:
 * - **Linux**: Uses WebKit extension (mmap, direct)
 * - **Android**: Uses invoke command (for small data)
 * - **Windows**: Uses SharedBuffer API
 * 
 * @param bufferName - Name of the memio buffer
 * @param data - Data to write
 * @returns MemioWriteResult with success status and metadata
 */
export async function memioWrite(
  bufferName: string,
  data: Uint8Array | ArrayBuffer
): Promise<MemioWriteResult> {
  const platform = detectPlatform();
  const bytes = data instanceof ArrayBuffer ? new Uint8Array(data) : data;
  const start = performance.now();
  
  switch (platform) {
    case 'linux':
      return writeLinux(bufferName, bytes, start);
    case 'android':
      // For Android, we can't write directly without Base64
      // The frontend should use memioUpload with a file URI instead
      console.warn('[Memio] memioWrite on Android is slow. Consider using memioUpload with a file URI.');
      return writeViaInvoke(bufferName, bytes, start);
    case 'windows':
      return writeWindows(bufferName, bytes, start);
    default:
      throw new Error(`Unsupported platform: ${platform}`);
  }
}

/**
 * Upload a file to memio buffer.
 * 
 * This is the unified upload API optimized for files:
 * - **Linux**: Reads file and writes via WebKit extension
 * - **Android**: Uses ContentResolver to read content:// URI natively (zero Base64)
 * - **Windows**: Uses SharedBuffer API
 * 
 * @param bufferName - Name of the memio buffer
 * @param fileUri - URI to the file (content:// on Android, file:// elsewhere)
 * @returns MemioWriteResult with success status and metadata
 */
export async function memioUpload(
  bufferName: string,
  fileUri: string
): Promise<MemioWriteResult> {
  // On all platforms, use the unified backend command
  const result = await invoke<{
    success: boolean;
    bytesWritten: number;
    version: number;
    durationMs: number;
  }>('plugin:memio|memio_upload', {
    bufferName,
    fileUri,
  });
  
  return {
    success: result.success,
    bytesWritten: result.bytesWritten,
    version: BigInt(result.version),
    durationMs: result.durationMs,
  };
}

/**
 * Upload a File object to memio region with automatic platform handling.
 *
 * - **Android**: Uses memio_upload with a content:// URI captured by the file picker.
 * - **Desktop**: Reads the File as bytes and writes to Memio directly.
 */
export async function memioUploadFile(
  bufferName: string,
  file: File,
  fileUri?: string
): Promise<MemioWriteResult> {
  const platform = detectPlatform();
  const start = performance.now();

  if (platform === 'android') {
    const global = globalThis as any;
    const uri = fileUri ?? global.__MEMIO_FILE_URIS__?.file_0;
    if (!uri) {
      throw new Error('Android file URI not available. Pick the file again.');
    }
    return memioUpload(bufferName, uri);
  }

  if (platform === 'windows') {
    const { uploadFileViaSharedBufferStream, shouldStreamUpload } = await import('./platform/windows');
    if (shouldStreamUpload(file.size)) {
      const version = Date.now();
      const success = await uploadFileViaSharedBufferStream(bufferName, file, version);
      return {
        success,
        bytesWritten: success ? file.size : 0,
        version: BigInt(version),
        durationMs: performance.now() - start,
      };
    }
  }

  const buffer = await file.arrayBuffer();
  const bytes = new Uint8Array(buffer);
  const success = await writeMemioSharedBuffer(bufferName, bytes);

  return {
    success,
    bytesWritten: success ? bytes.length : 0,
    version: BigInt(Date.now()),
    durationMs: performance.now() - start,
  };
}

// =============================================================================
// Platform-specific implementations
// =============================================================================

async function readLinux(bufferName: string, lastVersion?: bigint): Promise<MemioReadResult | null> {
  // Linux uses WebKit extension injected memioSharedBuffer
  if (typeof window.memioSharedBuffer !== 'function') {
    console.error('[Memio] Linux: memioSharedBuffer not available');
    return null;
  }
  
  const buffer = window.memioSharedBuffer(bufferName);
  if (!buffer) {
    return null;
  }
  
  const bytes = new Uint8Array(buffer);
  
  // Parse header: [magic:8][version:8][length:8][reserved:40][data...]
  const HEADER_SIZE = 64;
  const view = new DataView(buffer);
  const version = view.getBigUint64(8, true);
  const length = Number(view.getBigUint64(16, true));
  
  // Check version
  if (lastVersion !== undefined && version <= lastVersion) {
    return null;
  }
  
  // Extract data portion only
  const data = bytes.subarray(HEADER_SIZE, HEADER_SIZE + length);
  
  return {
    data,
    version,
    length,
    view: new StateView(data),
  };
}

async function readAndroid(bufferName: string, lastVersion?: bigint): Promise<MemioReadResult | null> {
  // Android uses memio:// protocol
  const result = await readSharedStateAndroidMemioProtocol(bufferName, lastVersion);
  if (!result) {
    return null;
  }
  
  // Get the underlying bytes from StateView
  const data = result.view.bytes;
  
  return {
    data,
    version: result.version,
    length: result.length,
    view: result.view,
  };
}

async function readWindows(bufferName: string, lastVersion?: bigint): Promise<MemioReadResult | null> {
  // Windows uses SharedBuffer via downloadViaSharedBuffer
  const { downloadViaSharedBuffer, hasSharedBufferDownload } = await import('./platform/windows');
  
  if (!hasSharedBufferDownload()) {
    // Fall back to invoke - data not directly accessible
    console.warn('[Memio] Windows SharedBuffer not available');
    return null;
  }
  
  const result = await downloadViaSharedBuffer(bufferName);
  if (!result) {
    return null;
  }
  
  // Check version
  if (lastVersion !== undefined && result.version <= lastVersion) {
    return null;
  }
  
  return {
    data: result.data,
    version: result.version,
    length: result.data.length,
    view: new StateView(result.data),
  };
}

async function writeLinux(
  bufferName: string,
  data: Uint8Array,
  start: number
): Promise<MemioWriteResult> {
  if (typeof window.memioWriteSharedBuffer !== 'function') {
    throw new Error('Linux: memioWriteSharedBuffer not available');
  }
  
  const success = window.memioWriteSharedBuffer(bufferName, data);
  
  return {
    success,
    bytesWritten: success ? data.length : 0,
    version: BigInt(Date.now()),
    durationMs: performance.now() - start,
  };
}

async function writeWindows(
  bufferName: string,
  data: Uint8Array,
  start: number
): Promise<MemioWriteResult> {
  const { uploadViaSharedBuffer, hasSharedBufferUpload } = await import('./platform/windows');
  
  if (!hasSharedBufferUpload()) {
    throw new Error('Windows SharedBuffer upload not available');
  }
  
  const version = Date.now();
  const success = await uploadViaSharedBuffer(bufferName, data, version);
  
  return {
    success,
    bytesWritten: success ? data.length : 0,
    version: BigInt(version),
    durationMs: performance.now() - start,
  };
}

async function writeViaInvoke(
  bufferName: string,
  data: Uint8Array,
  start: number
): Promise<MemioWriteResult> {
  // Fallback: send data via IPC (requires Base64 on Android)
  await invoke<{ success: boolean; bytesWritten: number; version: number }>(
    'upload_file_ready',
    { bufferName, data: Array.from(data), size: data.length }
  );
  
  return {
    success: true,
    bytesWritten: data.length,
    version: BigInt(Date.now()),
    durationMs: performance.now() - start,
  };
}
