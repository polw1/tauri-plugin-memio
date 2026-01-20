/**
 * Windows platform integration using WebView2 SharedBuffer API.
 * 
 * This module provides direct data transfer between JavaScript and Rust
 * using WebView2's PostSharedBufferToScript API.
 * 
 * Upload (Front→Back): JS writes to SharedBuffer, Rust reads
 * Download (Back→Front): Rust writes to SharedBuffer, JS reads
 */

import { invoke } from '@tauri-apps/api/core';

/** Ensures we only wire the SharedBuffer listener once. */
let listenerInitialized = false;

const LARGE_FILE_THRESHOLD = 100 * 1024 * 1024;
const STREAM_CHUNK_SIZE = 10 * 1024 * 1024;

/**
 * Checks if running on Windows platform.
 */
export function isWindowsPlatform(): boolean {
  if (typeof navigator === 'undefined') return false;
  return /Windows/.test(navigator.userAgent);
}

/**
 * Checks if Windows WebView2 SharedBuffer API is available.
 */
export function hasWindowsSharedBuffer(): boolean {
  if (
    typeof window !== 'undefined' &&
    'chrome' in window &&
    // @ts-expect-error - WebView2 API
    window.chrome?.webview !== undefined
  ) {
    return true;
  }
  
  return isWindowsPlatform();
}

// ============================================================================
// SharedBuffer State Management
// ============================================================================

/**
 * Pending upload buffers received from Rust.
 * These are SharedBuffers with ReadWrite access for direct uploads.
 */
const pendingUploadBuffers = new Map<string, ArrayBuffer>();
const pendingControlBuffers = new Map<string, ArrayBuffer>();

/**
 * Pending download buffers received from Rust.
 * These are SharedBuffers with ReadOnly access for direct downloads.
 */
const pendingDownloadBuffers = new Map<string, { buffer: ArrayBuffer; version: bigint; size: number }>();

/**
 * Resolvers waiting for download buffers.
 */
const pendingDownloadResolvers = new Map<string, Array<(result: { buffer: ArrayBuffer; version: bigint }) => void>>();

// ============================================================================
// Backend region helpers
// ============================================================================

/**
 * Ensure the memio region exists on the backend before we try to write into it.
 * Falls back gracefully if the command is unavailable.
 */
async function ensureMemioRegionExists(name: string, size: number): Promise<boolean> {
  try {
    const has = await invoke<boolean>('plugin:memio|has_shared_buffer', { name });
    if (has) return true;

    await invoke('plugin:memio|create_shared_buffer_windows', { name, size });
    return true;
  } catch (error) {
    console.warn('[MemioWindows] Failed to ensure memio region exists:', error);
    return false;
  }
}

// ============================================================================
// Initialization
// ============================================================================

/**
 * Initializes the Windows SharedBuffer listener.
 * Call this once at application startup.
 */
export function initWindowsSharedBuffer(): void {
  if (listenerInitialized) return;

  if (!hasWindowsSharedBuffer()) {
    console.warn('[MemioWindows] WebView2 SharedBuffer not available');
    return;
  }

  listenerInitialized = true;

  // @ts-expect-error - WebView2 API
  window.chrome.webview.addEventListener('sharedbufferreceived', (event: any) => {
    try {
      // Parse metadata - can be string or object depending on WebView2 version
      let metadata: {
        name: string;
        bufferName?: string;
        version?: number;
        size?: number;
        forUpload?: boolean;
        forUploadControl?: boolean;
        forDownload?: boolean;
      };
      
      const additionalData = event.additionalData;
      if (typeof additionalData === 'string') {
        metadata = JSON.parse(additionalData || '{}');
      } else if (typeof additionalData === 'object' && additionalData !== null) {
        metadata = additionalData;
      } else {
        metadata = { name: 'unknown' };
      }
      
      console.debug('[MemioWindows] SharedBuffer event:', metadata);

      // Handle upload buffers (JS writes, Rust reads)
      if (metadata.forUpload) {
        const buffer: ArrayBuffer = event.getBuffer();
        pendingUploadBuffers.set(metadata.name, buffer);
        console.debug(`[MemioWindows] Upload buffer ready: ${metadata.name} (${metadata.size} bytes)`);
        // DON'T close - we need to write to it!
        return;
      }

      if (metadata.forUploadControl) {
        const buffer: ArrayBuffer = event.getBuffer();
        pendingControlBuffers.set(metadata.name, buffer);
        console.debug(`[MemioWindows] Upload control buffer ready: ${metadata.name} (${metadata.size} bytes)`);
        // DON'T close - we need to write to it!
        return;
      }

      // Handle download buffers (Rust writes, JS reads)
      if (metadata.forDownload) {
        const buffer: ArrayBuffer = event.getBuffer();
        const name = metadata.name;
        const version = BigInt(metadata.version ?? 0);
        const size = metadata.size ?? buffer.byteLength;
        
        console.debug(`[MemioWindows] Download buffer received: ${name} (${size} bytes, v${version})`);
        
        // Store for consumption
        pendingDownloadBuffers.set(name, { buffer, version, size });
        
        // Resolve any pending promises
        const resolvers = pendingDownloadResolvers.get(name);
        if (resolvers) {
          for (const resolve of resolvers) {
            resolve({ buffer, version });
          }
          pendingDownloadResolvers.delete(name);
        }
        
        // Close the buffer handle
        if (typeof event.close === 'function') {
          event.close();
        }
        return;
      }

      // Unknown buffer type
      console.warn('[MemioWindows] Unknown SharedBuffer type:', metadata);
      if (typeof event.close === 'function') {
        event.close();
      }
    } catch (error) {
      console.error('[MemioWindows] Error handling SharedBuffer event:', error);
    }
  });

  console.debug('[MemioWindows] SharedBuffer listener initialized');
}

/**
 * Safe bootstrap helper: initialize the listener only when WebView2 is present.
 * Call as early as possible in app startup.
 */
export function bootstrapWindowsSharedBuffer(): void {
  if (typeof window === 'undefined') return;
  if (!hasWindowsSharedBuffer()) return;
  initWindowsSharedBuffer();
}

// ============================================================================
// Direct Upload (Front → Back)
// ============================================================================

/**
 * Upload data using WebView2 SharedBuffer (direct).
 * 
 * Flow:
 * 1. Rust creates SharedBuffer and posts to JS (ReadWrite access)
 * 2. JS writes data directly to the SharedBuffer
 * 3. JS signals Rust to read from the SharedBuffer
 * 
 * @param name - Buffer name
 * @param data - Data to upload
 * @param version - Version number
 * @returns true if successful
 */
export async function uploadViaSharedBuffer(
  name: string,
  data: Uint8Array,
  version: number
): Promise<boolean> {
  try {
    console.debug(`[MemioWindows] Upload: ${data.length} bytes to '${name}'`);

    // Ensure the backend region exists before we start the SharedBuffer flow
    const regionReady = await ensureMemioRegionExists(name, data.length);
    if (!regionReady) {
      throw new Error('Memio region not available on backend');
    }
    
    // Step 1: Ask Rust to create SharedBuffer and post it to us
    const prepareResult = await invoke<{ name: string; size: number; ready: boolean }>(
      'plugin:memio|prepare_upload_buffer',
      { name, size: data.length }
    );
    
    if (!prepareResult.ready) {
      throw new Error('SharedBuffer not ready');
    }
    
    // Step 2: Wait for the SharedBuffer to arrive
    let uploadBuffer: ArrayBuffer | undefined;
    const maxWait = 100;
    const startWait = performance.now();
    
    while (!uploadBuffer && (performance.now() - startWait) < maxWait) {
      uploadBuffer = pendingUploadBuffers.get(name);
      if (!uploadBuffer) {
        await new Promise(r => setTimeout(r, 1));
      }
    }
    
    if (!uploadBuffer) {
      throw new Error(`Upload buffer '${name}' not received`);
    }
    
    // Step 3: Write data directly to the SharedBuffer (Direct!)
    const bufferView = new Uint8Array(uploadBuffer);
    bufferView.set(data);
    
    // Step 4: Commit - tell Rust to read from the SharedBuffer
    await invoke('plugin:memio|commit_upload_buffer', {
      args: {
        name,
        version,
        length: data.length,
      },
    });
    
    // Clean up
    pendingUploadBuffers.delete(name);
    
    console.debug(`[MemioWindows] Upload complete: ${data.length} bytes (v${version})`);
    return true;
    
  } catch (error) {
    console.error('[MemioWindows] Upload failed:', error);
    return false;
  }
}

/**
 * Upload a File via SharedBuffer streaming (large files).
 * Uses a fixed SharedBuffer chunk and commits each chunk by offset.
 */
export async function uploadFileViaSharedBufferStream(
  name: string,
  file: File,
  version: number,
  chunkSize = STREAM_CHUNK_SIZE
): Promise<boolean> {
  try {
    console.debug(`[MemioWindows] Stream upload: ${file.size} bytes to '${name}'`);

    const regionReady = await ensureMemioRegionExists(name, file.size);
    if (!regionReady) {
      throw new Error('Memio region not available on backend');
    }

    const sharedSize = Math.min(chunkSize, file.size);
    const bufferCount = 8;

    const startResult = await invoke<{
      controlName: string;
      bufferNames: string[];
      capacity: number;
      entrySize: number;
    }>('plugin:memio|start_upload_stream', {
      args: {
        name,
        totalLength: file.size,
        chunkSize: sharedSize,
        bufferCount,
        version,
      },
    });

    const waitForBuffer = async (
      bufferName: string,
      map: Map<string, ArrayBuffer>,
      timeoutMs = 1000
    ): Promise<ArrayBuffer> => {
      const startWait = performance.now();
      while ((performance.now() - startWait) < timeoutMs) {
        const buffer = map.get(bufferName);
        if (buffer) return buffer;
        await new Promise(r => setTimeout(r, 1));
      }
      throw new Error(`Buffer '${bufferName}' not received`);
    };

    const controlBuffer = await waitForBuffer(startResult.controlName, pendingControlBuffers);
    const dataBuffers = await Promise.all(
      startResult.bufferNames.map(async (bufferName) => {
        const uploadBuffer = await waitForBuffer(bufferName, pendingUploadBuffers);
        return uploadBuffer;
      })
    );

    pendingControlBuffers.delete(startResult.controlName);
    for (const bufferName of startResult.bufferNames) {
      pendingUploadBuffers.delete(bufferName);
    }

    const workerSource = `
      self.onmessage = async (event) => {
        const { file, controlBuffer, dataBuffers, totalLength } = event.data;
        const sleep = (ms) => new Promise(r => setTimeout(r, ms));

        try {
          const controlView = new DataView(controlBuffer);
          const queueCapacity = controlView.getUint32(8, true);
          const entrySize = controlView.getUint32(12, true);
          if (!queueCapacity || !entrySize) {
            throw new Error('Invalid control buffer header');
          }

          const dataViews = dataBuffers.map((buf) => new Uint8Array(buf));
          const CONTROL_HEADER_SIZE = 16;
          const CONTROL_ENTRY_SIZE = entrySize;
          let tail = controlView.getUint32(4, true);

          const reader = file.stream().getReader();
          let pendingChunk = null;
          let pendingOffset = 0;
          let offset = 0;

          while (offset < totalLength) {
            let head = controlView.getUint32(0, true);
            while ((tail - head) >= queueCapacity) {
              await sleep(1);
              head = controlView.getUint32(0, true);
            }

            const bufferIndex = tail % dataViews.length;
            const dataView = dataViews[bufferIndex];
            const wanted = Math.min(dataView.byteLength, totalLength - offset);

            if (!pendingChunk || pendingOffset >= pendingChunk.byteLength) {
              const { value, done } = await reader.read();
              if (done || !value) break;
              pendingChunk = value;
              pendingOffset = 0;
            }

            if (!pendingChunk) break;

            const remaining = pendingChunk.byteLength - pendingOffset;
            const toCopy = Math.min(remaining, wanted);
            dataView.set(pendingChunk.subarray(pendingOffset, pendingOffset + toCopy), 0);
            pendingOffset += toCopy;

            if (toCopy === 0) break;

            const finalize = offset + toCopy >= totalLength;
            const entryIndex = tail % queueCapacity;
            const entryOffset = CONTROL_HEADER_SIZE + (entryIndex * CONTROL_ENTRY_SIZE);
            controlView.setUint32(entryOffset, bufferIndex, true);
            controlView.setUint32(entryOffset + 4, toCopy, true);
            controlView.setBigUint64(entryOffset + 8, BigInt(offset), true);
            controlView.setUint32(entryOffset + 16, finalize ? 1 : 0, true);
            controlView.setUint32(entryOffset + 20, 0, true);

            tail += 1;
            controlView.setUint32(4, tail, true);
            offset += toCopy;
          }

          try { reader.releaseLock(); } catch {}

          while (controlView.getUint32(0, true) !== tail) {
            await sleep(1);
          }

          self.postMessage({ status: 'ok', offset });
        } catch (error) {
          const message = error && error.message ? error.message : String(error);
          self.postMessage({ status: 'error', message });
        }
      };
    `;

    const workerUrl = URL.createObjectURL(new Blob([workerSource], { type: 'text/javascript' }));
    const worker = new Worker(workerUrl);

    const workerResult = new Promise<void>((resolve, reject) => {
      worker.onmessage = (event) => {
        const data = event.data;
        if (data && data.status === 'ok') {
          resolve();
        } else {
          reject(new Error(data?.message || 'Upload worker failed'));
        }
      };
      worker.onerror = (event) => {
        reject(new Error(event.message || 'Upload worker error'));
      };
    });

    try {
      worker.postMessage(
        {
          file,
          controlBuffer,
          dataBuffers,
          totalLength: file.size,
        },
        [controlBuffer, ...dataBuffers]
      );

      await workerResult;
    } finally {
      worker.terminate();
      URL.revokeObjectURL(workerUrl);
    }

    await invoke('plugin:memio|stop_upload_stream', { name });

    console.debug(`[MemioWindows] Stream upload complete: ${file.size} bytes (v${version})`);
    return true;
  } catch (error) {
    console.error('[MemioWindows] Stream upload failed:', error);
    return false;
  }
}

export function shouldStreamUpload(byteLength: number): boolean {
  return byteLength > LARGE_FILE_THRESHOLD;
}

/**
 * Check if SharedBuffer upload is available.
 */
export function hasSharedBufferUpload(): boolean {
  return hasWindowsSharedBuffer();
}

// ============================================================================
// Direct Download (Back → Front)
// ============================================================================

/**
 * Download data using WebView2 SharedBuffer (direct).
 * 
 * Flow:
 * 1. Rust reads data, creates SharedBuffer, writes data
 * 2. Rust posts buffer to JS (ReadOnly access)
 * 3. JS receives buffer via sharedbufferreceived event
 * 4. JS reads data from the SharedBuffer
 * 
 * @param name - Buffer name
 * @param timeoutMs - Timeout in milliseconds (default: 5000)
 * @returns { version, data } or null on failure
 */
export async function downloadViaSharedBuffer(
  name: string,
  timeoutMs = 5000
): Promise<{ version: bigint; data: Uint8Array } | null> {
  try {
    console.debug(`[MemioWindows] Download: requesting '${name}'`);
    
    // Check if we already have this buffer cached
    const cached = pendingDownloadBuffers.get(name);
    if (cached) {
      pendingDownloadBuffers.delete(name);
      const data = new Uint8Array(cached.buffer, 0, cached.size);
      console.debug(`[MemioWindows] Download from cache: ${data.length} bytes`);
      return { version: cached.version, data };
    }
    
    // Create promise to wait for the buffer
    const bufferPromise = new Promise<{ buffer: ArrayBuffer; version: bigint }>((resolve, reject) => {
      const resolvers = pendingDownloadResolvers.get(name) || [];
      
      const timeout = setTimeout(() => {
        pendingDownloadResolvers.delete(name);
        reject(new Error(`Timeout waiting for buffer: ${name}`));
      }, timeoutMs);
      
      resolvers.push((result) => {
        clearTimeout(timeout);
        resolve(result);
      });
      
      pendingDownloadResolvers.set(name, resolvers);
    });
    
    // Step 1: Ask Rust to read data and post it to us via SharedBuffer
    const sendResult = await invoke<{ name: string; version: number; size: number; posted: boolean }>(
      'plugin:memio|send_download_buffer',
      { name }
    );
    
    if (!sendResult.posted) {
      throw new Error('Rust failed to post download buffer');
    }
    
    console.debug(`[MemioWindows] Rust posted: ${sendResult.size} bytes (v${sendResult.version})`);
    
    // Step 2: Wait for the SharedBuffer to arrive
    const { buffer, version } = await bufferPromise;
    
    // Step 3: Read data from the SharedBuffer
    const data = new Uint8Array(buffer, 0, sendResult.size);
    
    console.debug(`[MemioWindows] Download complete: ${data.length} bytes (v${version})`);
    
    return { version, data };
    
  } catch (error) {
    console.error('[MemioWindows] Download failed:', error);
    return null;
  }
}

/**
 * Check if SharedBuffer download is available.
 */
export function hasSharedBufferDownload(): boolean {
  return hasWindowsSharedBuffer();
}
