/**
 * Android platform implementation - ZERO BASE64!
 * 
 * ## Read: memio:// protocol
 * Uses MemioWebViewClient.shouldInterceptRequest() to serve raw bytes
 * from DirectByteBuffer via WebResourceResponse.
 * 
 * ## Write: upload_file_from_uri command
 * Uses MemioPlugin.uploadFileFromUri() with ContentResolver to read
 * content:// URIs natively without Base64.
 * 
 */

import { StateView } from '../state-view';
import type { SharedStateSnapshot, SharedStateWriteResult, MemioAndroidGlobals } from '../shared-types';

export function hasAndroidBridge(): boolean {
  const global = globalThis as unknown as MemioAndroidGlobals;
  // Only check MemioNative - __memioSharedManifest is also created by Linux WebKit extension
  return typeof global.MemioNative !== 'undefined';
}

/**
 * Version check - only reads 8 bytes from memio region.
 * Use this when you just want to poll for changes.
 */
export function getVersionAndroid(name?: string): bigint {
  const global = globalThis as unknown as MemioAndroidGlobals;
  const bufferName = name || 'state';

  // Try the injected helper first
  if (typeof global.memioGetVersion === 'function') {
    const version = global.memioGetVersion(bufferName);
    return BigInt(version);
  }

  // Fall back to direct MemioNative call
  if (global.MemioNative) {
    const version = global.MemioNative.getVersion(bufferName);
    return BigInt(version);
  }

  return BigInt(-1);
}

/**
 * Read memio buffer using memio:// protocol (ZERO BASE64!)
 * 
 * This is an async operation that uses XMLHttpRequest to fetch data
 * from MemioWebViewClient, which returns raw bytes via WebResourceResponse.
 * 
 * Returns: { data: ArrayBuffer, version: bigint, length: number }
 */
export async function getAndroidSharedBufferAsync(name?: string): Promise<{
  data: ArrayBuffer;
  version: bigint;
  length: number;
} | null> {
  const bufferName = name || 'state';
  
  try {
    const url = `memio://buffer/${bufferName}`;
    
    const result = await new Promise<{ data: ArrayBuffer; version: bigint; length: number }>((resolve, reject) => {
      const xhr = new XMLHttpRequest();
      xhr.open('GET', url, true);
      xhr.responseType = 'arraybuffer';
      
      xhr.onload = () => {
        if (xhr.status === 200) {
          // Read version and length from response headers
          const versionStr = xhr.getResponseHeader('X-Memio-Version');
          const lengthStr = xhr.getResponseHeader('X-Memio-Length');
          
          const version = versionStr ? BigInt(versionStr) : BigInt(0);
          const length = lengthStr ? parseInt(lengthStr, 10) : xhr.response.byteLength;
          
          resolve({
            data: xhr.response,
            version,
            length
          });
        } else {
          reject(new Error(`HTTP ${xhr.status}: ${xhr.statusText}`));
        }
      };
      
      xhr.onerror = () => reject(new Error('Network error'));
      xhr.send();
    });
    
    return result;
  } catch (error) {
    console.error('[MemioAndroid] getAndroidSharedBufferAsync error:', error);
    return null;
  }
}

/**
 * Synchronous version - returns null on Android (use async version).
 * Kept for interface compatibility.
 */
export function getAndroidSharedBuffer(_name?: string): ArrayBuffer | Uint8Array | null {
  // On Android, we can't do synchronous reads without Base64.
  // Use getAndroidSharedBufferAsync() or readSharedStateAndroidAsync() instead.
  console.warn('[MemioAndroid] getAndroidSharedBuffer is deprecated. Use async version.');
  return null;
}

/**
 * Read shared state using memio:// protocol (ZERO BASE64!)
 * 
 * @param name The name of the memio region
 * @param lastVersion Optional - skip read if version hasn't changed
 * @returns SharedStateSnapshot or null on error/no change
 */
export async function readSharedStateAndroidAsync(
  name?: string,
  lastVersion?: bigint
): Promise<SharedStateSnapshot | null> {
  const bufferName = name || 'state';

  // Version check first (avoids full read if no change)
  if (lastVersion !== undefined && lastVersion >= 0) {
    const currentVersion = getVersionAndroid(bufferName);
    if (currentVersion <= lastVersion) {
      return null; // No change
    }
  }

  const result = await getAndroidSharedBufferAsync(bufferName);
  if (!result || !result.data) {
    return null;
  }

  const bytes = new Uint8Array(result.data);
  
  return {
    version: result.version,
    length: result.length,
    view: new StateView(bytes),
  };
}

/**
 * Synchronous read - uses version check, returns null on Android.
 * For actual data, use readSharedStateAndroidAsync().
 * Kept for interface compatibility.
 */
export function readSharedStateAndroid(
  name?: string,
  lastVersion?: bigint
): SharedStateSnapshot | null {
  // On Android, synchronous reads require Base64 which we've removed.
  // Use readSharedStateAndroidAsync() instead.
  
  // We can still do version checks synchronously
  if (lastVersion !== undefined && lastVersion >= 0) {
    const currentVersion = getVersionAndroid(name);
    if (currentVersion <= lastVersion) {
      return null; // No change
    }
  }
  
  console.warn('[MemioAndroid] readSharedStateAndroid is deprecated. Use async version.');
  return null;
}

/**
 * Write to memio region on Android.
 * 
 * For file uploads, use the upload_file_from_uri command instead:
 * ```ts
 * await invoke('upload_file_from_uri', { fileUri, bufferName })
 * ```
 * 
 * This method is kept for interface compatibility but is deprecated
 * since it would require Base64 encoding.
 */
export function writeSharedStateAndroid(
  _name: string = 'state',
  _data: ArrayBuffer | Uint8Array,
  _version?: bigint
): SharedStateWriteResult | null {
  // On Android, writing via JS bridge requires Base64 which we've removed.
  // Use the upload_file_from_uri command for file uploads instead.
  console.warn('[MemioAndroid] writeSharedStateAndroid is deprecated. Use upload_file_from_uri command.');
  return null;
}
