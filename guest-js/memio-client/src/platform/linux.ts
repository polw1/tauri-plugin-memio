import type { MemioLinuxGlobals } from '../shared-types';

export function hasLinuxSharedMemory(): boolean {
  const global = globalThis as unknown as MemioLinuxGlobals;
  if (typeof global.memioSharedBuffer === 'function' || global.__memioSharedBuffers) {
    return true;
  }
  if (typeof global.__memioSharedPath === 'string' || typeof global.__memioSharedRegistryPath === 'string') {
    return true;
  }
  if (global.__memioSharedManifest && typeof global.__memioSharedManifest === 'object') {
    return true;
  }
  return false;
}

export function getLinuxSharedBuffer(name?: string): ArrayBuffer | Uint8Array | null {
  const global = globalThis as unknown as MemioLinuxGlobals;
  const bufferName = name || 'state';

  if (typeof global.memioSharedBuffer === 'function') {
    return global.memioSharedBuffer(bufferName) ?? null;
  }

  if (global.__memioSharedBuffers) {
    return global.__memioSharedBuffers[bufferName] ?? null;
  }

  return null;
}
