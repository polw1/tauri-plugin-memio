/**
 * MemioClient - Memio region access for MemioTauri applications.
 *
 * @module memio-client
 */

import { createMemioClient, MemioClient, type MemioConfig } from './client';
import { StateView } from './state-view';
import type { SharedStateSnapshot, SharedStateWriteResult, MemioPlatform, SharedStateManifest } from './shared-state';

export interface MemioSimpleClient {
  shared(lastVersion?: bigint): SharedStateSnapshot | null;
  sharedNamed(name: string, lastVersion?: bigint): SharedStateSnapshot | null;
  writeShared(data: ArrayBuffer | Uint8Array, version?: bigint): SharedStateWriteResult | null;
  writeSharedNamed(name: string, data: ArrayBuffer | Uint8Array, version?: bigint): SharedStateWriteResult | null;
  sharedBuffer(name: string): Uint8Array | null;
  sharedManifest(): SharedStateManifest | null;
  /** Returns the detected platform (linux, android, ios, macos, windows, unknown) */
  platform(): MemioPlatform;
  /** Returns true if memio region is available on this platform */
  hasSharedMemory(): boolean;
  /** Waits for memio region to become available */
  waitForSharedMemory(name?: string, timeoutMs?: number): Promise<ArrayBuffer | Uint8Array | null>;
  raw(): MemioClient;
}

export async function memio(config?: MemioConfig): Promise<MemioSimpleClient> {
  const client = createMemioClient(config);
  return {
    shared: (lastVersion?: bigint) => client.readSharedState(lastVersion),
    sharedNamed: (name: string, lastVersion?: bigint) => client.readSharedStateNamed(name, lastVersion),
    writeShared: (data: ArrayBuffer | Uint8Array, version?: bigint) => client.writeSharedState(data, version),
    writeSharedNamed: (name: string, data: ArrayBuffer | Uint8Array, version?: bigint) =>
      client.writeSharedStateNamed(name, data, version),
    sharedBuffer: (name: string) => client.readSharedBuffer(name),
    sharedManifest: () => client.sharedManifest(),
    platform: () => client.detectPlatform(),
    hasSharedMemory: () => client.isSharedMemoryAvailable(),
    waitForSharedMemory: (name?: string, timeoutMs?: number) => client.waitForSharedMemory(name, timeoutMs),
    raw: () => client,
  };
}

export { MemioClient, createMemioClient };
export { StateView };
export { createSharedStateProvider } from './provider';
export type { SharedStateProvider } from './provider';
export { 
  getMemioSharedBuffer,
  writeMemioSharedBuffer,
  readMemioSharedBuffer,
  waitForSharedBuffer, 
  readSharedState,
  writeSharedStateBuffer,
  readSharedStateAndroid,
  getSharedManifest,
  detectPlatform,
  isSharedMemoryAvailable, 
  SHARED_STATE_HEADER_SIZE, 
  SHARED_STATE_MAGIC 
} from './shared-state';
export type { SharedStateManifest, SharedStateSnapshot, SharedStateWriteResult, MemioPlatform, MemioConfig };

// =============================================================================
// UNIFIED API - Use these for cross-platform experience
// =============================================================================
export { memioRead, memioWrite, memioUpload, memioUploadFile } from './unified';
export type { MemioReadResult, MemioWriteResult } from './unified';
