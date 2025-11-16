import { StateView } from './state-view';
import {
  SHARED_STATE_MAGIC,
  SHARED_STATE_HEADER_SIZE,
  SHARED_STATE_LENGTH_OFFSET,
  SHARED_STATE_MAGIC_OFFSET,
  SHARED_STATE_VERSION_OFFSET,
} from './shared-state-spec';
import { SHARED_MANIFEST_VERSION } from './shared-manifest-spec';
import { getLinuxSharedBuffer, hasLinuxSharedMemory } from './platform/linux';
import type {
  SharedStateManifest,
  SharedStateSnapshot,
  SharedStateWriteResult,
  MemioPlatform,
  MemioGlobalBase,
} from './shared-types';

/**
 * Detects the current platform based on available globals.
 */
export function detectPlatform(): MemioPlatform {
  if (hasLinuxSharedMemory()) {
    return 'linux';
  }
  const ua = typeof navigator !== 'undefined' ? navigator.userAgent : '';
  if (/Linux/.test(ua)) {
    return 'linux';
  }
  return 'unknown';
}

/**
 * Checks if shared memory is available on the current platform.
 */
export function isSharedMemoryAvailable(): boolean {
  return hasLinuxSharedMemory();
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
  return getLinuxSharedBuffer(name);
}

/**
 * Reads data from shared memory buffer.
 */
export async function readMemioSharedBuffer(
  name: string
): Promise<{ version: bigint; data: Uint8Array } | null> {
  const buffer = getMemioSharedBuffer(name);
  if (!buffer) {
    return null;
  }

  const bytes = buffer instanceof Uint8Array ? buffer : new Uint8Array(buffer);
  if (bytes.length < SHARED_STATE_HEADER_SIZE) {
    console.warn(`[MemioClient] Linux: buffer too small (${bytes.length} < ${SHARED_STATE_HEADER_SIZE})`);
    return null;
  }

  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const version = view.getBigUint64(SHARED_STATE_VERSION_OFFSET, true);
  const length = Number(view.getBigUint64(SHARED_STATE_LENGTH_OFFSET, true));
  const data = bytes.subarray(SHARED_STATE_HEADER_SIZE, SHARED_STATE_HEADER_SIZE + length);

  console.log(`[MemioClient] Linux: read ${data.length} bytes from '${name}' (version ${version})`);
  return { version, data };
}

/**
 * Writes data directly to shared memory buffer.
 */
export async function writeMemioSharedBuffer(
  name: string,
  data: Uint8Array | ArrayBuffer,
  timeoutMs = 5000
): Promise<boolean> {
  const global = globalThis as any;
  const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);

  if (typeof global.memioWriteSharedBuffer !== 'function') {
    console.warn('[MemioClient] memioWriteSharedBuffer not available - Linux WebKit extension required');
    return false;
  }

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
      console.debug(`[MemioClient] Write attempt ${attempts} failed, retrying...`, error);
    }

    await new Promise(resolve => setTimeout(resolve, 100));
  }

  console.error(`[MemioClient] Failed to write to buffer '${name}' after ${attempts} attempts (${timeoutMs}ms). Backend must create buffer first with manager.create_buffer("${name}", size)`);
  return false;
}

/**
 * Waits for the shared buffer to become available.
 */
export async function waitForSharedBuffer(
  name?: string,
  timeoutMs = 2000
): Promise<ArrayBuffer | Uint8Array | null> {
  const startTime = Date.now();
  const bufferName = name ?? 'state';

  while (Date.now() - startTime < timeoutMs) {
    if (hasLinuxSharedMemory()) {
      if (typeof (globalThis as any).memioSharedBuffer === 'function') {
        return new Uint8Array(0);
      }
      const buffer = getMemioSharedBuffer(bufferName);
      if (buffer) {
        return buffer;
      }
    }

    await new Promise(resolve => setTimeout(resolve, 50));
  }

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
