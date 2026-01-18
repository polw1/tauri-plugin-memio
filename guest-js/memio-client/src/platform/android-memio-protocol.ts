/**
 * Android memio:// protocol implementation - ZERO BASE64!
 * 
 * Uses custom URL scheme handler to fetch data from the memio region
 * without Base64 encoding/decoding overhead.
 * 
 * ## Architecture:
 * ```
 * READ:  JS fetch("memio://buffer/name") → MemioWebViewClient → DirectByteBuffer → raw bytes
 * WRITE: JS invoke("upload_file_from_uri") → MemioPlugin → ContentResolver → MemioSharedMemory
 * ```
 * 
 */

import { StateView } from '../state-view';
import type { SharedStateSnapshot } from '../shared-types';
import { getVersionAndroid } from './android';

/**
 * Read shared state using memio:// protocol (ZERO BASE64!)
 * 
 * This uses XMLHttpRequest with a custom URL scheme that's intercepted by
 * MemioWebViewClient.shouldInterceptRequest(), which reads from
 * DirectByteBuffer and returns raw bytes via WebResourceResponse.
 * 
 * Version and length are returned in HTTP headers (X-Memio-Version, X-Memio-Length)
 * to avoid sending the 64-byte header in the response body.
 * 
 * @param name The name of the memio region
 * @param lastVersion Optional - skip read if version hasn't changed
 * @returns SharedStateSnapshot or null on error/no change
 */
export async function readSharedStateAndroidMemioProtocol(
  name: string = 'state',
  lastVersion?: bigint
): Promise<SharedStateSnapshot | null> {
  // Version check first
  if (lastVersion !== undefined && lastVersion >= 0) {
    const currentVersion = getVersionAndroid(name);
    if (currentVersion <= lastVersion) {
      return null; // No change
    }
  }

  try {
    const startTime = performance.now();
    const url = `memio://buffer/${name}`;
    
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
          
          resolve({ data: xhr.response, version, length });
        } else {
          reject(new Error(`HTTP ${xhr.status}: ${xhr.statusText}`));
        }
      };
      
      xhr.onerror = () => reject(new Error('Network error'));
      xhr.send();
    });
    
    const bytes = new Uint8Array(result.data);
    const elapsed = performance.now() - startTime;
    console.log(`[MemioAndroid] memio:// read ${bytes.length} bytes in ${elapsed.toFixed(2)}ms (ZERO BASE64! 🚀)`);
    
    return {
      version: result.version,
      length: result.length,
      view: new StateView(bytes)
    };
  } catch (error) {
    console.error('[MemioAndroid] Error reading via memio:// protocol:', error);
    return null;
  }
}
