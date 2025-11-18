/**
 * Unified Memio API (Linux)
 */

import { invoke } from '@tauri-apps/api/core';
import { detectPlatform, writeMemioSharedBuffer } from './shared-state';
import { StateView } from './state-view';

export interface MemioReadResult {
  data: Uint8Array;
  version: bigint;
  length: number;
  view: StateView;
}

export interface MemioWriteResult {
  success: boolean;
  bytesWritten: number;
  version: bigint;
  durationMs: number;
}

declare global {
  interface Window {
    memioSharedBuffer?: (name: string) => ArrayBuffer | null;
    memioWriteSharedBuffer?: (name: string, data: Uint8Array) => boolean;
  }
}

export async function memioRead(
  bufferName: string = 'state',
  lastVersion?: bigint
): Promise<MemioReadResult | null> {
  const platform = detectPlatform();
  if (platform !== 'linux') {
    console.warn(`[Memio] Unsupported platform: ${platform}`);
    return null;
  }
  return readLinux(bufferName, lastVersion);
}

export async function memioWrite(
  bufferName: string,
  data: Uint8Array | ArrayBuffer
): Promise<MemioWriteResult> {
  const platform = detectPlatform();
  if (platform !== 'linux') {
    throw new Error(`Unsupported platform: ${platform}`);
  }
  const bytes = data instanceof ArrayBuffer ? new Uint8Array(data) : data;
  const start = performance.now();
  return writeLinux(bufferName, bytes, start);
}

export async function memioUpload(
  bufferName: string,
  fileUri: string
): Promise<MemioWriteResult> {
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

export async function memioUploadFile(
  bufferName: string,
  file: File
): Promise<MemioWriteResult> {
  const buffer = await file.arrayBuffer();
  const bytes = new Uint8Array(buffer);
  const start = performance.now();
  const success = await writeMemioSharedBuffer(bufferName, bytes);

  return {
    success,
    bytesWritten: success ? bytes.length : 0,
    version: BigInt(Date.now()),
    durationMs: performance.now() - start,
  };
}

async function readLinux(bufferName: string, lastVersion?: bigint): Promise<MemioReadResult | null> {
  if (typeof window.memioSharedBuffer !== 'function') {
    console.error('[Memio] Linux: memioSharedBuffer not available');
    return null;
  }

  const buffer = window.memioSharedBuffer(bufferName);
  if (!buffer) {
    return null;
  }

  const bytes = new Uint8Array(buffer);
  const HEADER_SIZE = 64;
  const view = new DataView(buffer);
  const version = view.getBigUint64(8, true);
  const length = Number(view.getBigUint64(16, true));

  if (lastVersion !== undefined && version <= lastVersion) {
    return null;
  }

  const data = bytes.subarray(HEADER_SIZE, HEADER_SIZE + length);

  return {
    data,
    version,
    length,
    view: new StateView(data),
  };
}

async function writeLinux(bufferName: string, bytes: Uint8Array, start: number): Promise<MemioWriteResult> {
  const success = await writeMemioSharedBuffer(bufferName, bytes);
  return {
    success,
    bytesWritten: success ? bytes.length : 0,
    version: BigInt(Date.now()),
    durationMs: performance.now() - start,
  };
}
