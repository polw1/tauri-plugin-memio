import { getLinuxSharedBuffer, hasLinuxSharedMemory } from './platform/linux';
import type { SharedStateSnapshot, SharedStateWriteResult, MemioPlatform } from './shared-types';
import type { SharedStateManifest } from './shared-types';
import { readSharedState, writeSharedStateBuffer, getSharedManifest } from './shared-state';

export interface SharedStateProvider {
  platform(): MemioPlatform;
  isAvailable(): boolean;
  sharedManifest(): SharedStateManifest | null;
  getSharedBuffer(name?: string): ArrayBuffer | Uint8Array | null;
  readSharedState(name?: string, lastVersion?: bigint): SharedStateSnapshot | null;
  writeSharedState(name: string, data: ArrayBuffer | Uint8Array, version?: bigint): SharedStateWriteResult | null;
}

class LinuxProvider implements SharedStateProvider {
  platform(): MemioPlatform {
    return 'linux';
  }

  isAvailable(): boolean {
    return hasLinuxSharedMemory();
  }

  sharedManifest(): SharedStateManifest | null {
    return getSharedManifest();
  }

  getSharedBuffer(name?: string): ArrayBuffer | Uint8Array | null {
    return getLinuxSharedBuffer(name);
  }

  readSharedState(name?: string, lastVersion?: bigint): SharedStateSnapshot | null {
    const buffer = getLinuxSharedBuffer(name);
    if (!buffer) {
      return null;
    }
    return readSharedState(buffer, lastVersion);
  }

  writeSharedState(name: string, data: ArrayBuffer | Uint8Array, version?: bigint): SharedStateWriteResult | null {
    const buffer = getLinuxSharedBuffer(name);
    if (!buffer) {
      return null;
    }
    return writeSharedStateBuffer(buffer, data, version);
  }
}

class UnknownProvider implements SharedStateProvider {
  platform(): MemioPlatform {
    return 'unknown';
  }

  isAvailable(): boolean {
    return false;
  }

  sharedManifest(): SharedStateManifest | null {
    return null;
  }

  getSharedBuffer(): ArrayBuffer | Uint8Array | null {
    return null;
  }

  readSharedState(): SharedStateSnapshot | null {
    return null;
  }

  writeSharedState(): SharedStateWriteResult | null {
    return null;
  }
}

export function createSharedStateProvider(): SharedStateProvider {
  if (hasLinuxSharedMemory()) {
    return new LinuxProvider();
  }
  return new UnknownProvider();
}
