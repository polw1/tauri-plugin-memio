/**
 * MemioClient - Shared memory access for MemioTauri applications.
 */

import {
  detectPlatform,
  isSharedMemoryAvailable,
  waitForSharedBuffer,
  type SharedStateSnapshot,
  type MemioPlatform,
} from './shared-state';
import type { SharedStateManifest, SharedStateWriteResult } from './shared-types';
import { createSharedStateProvider, type SharedStateProvider } from './provider';

export interface MemioConfig {
  /**
   * Default shared buffer name.
   * @default 'state'
   */
  bufferName?: string;

  /**
   * Enable debug logging.
   * @default false
   */
  debug?: boolean;
}

export class MemioClient {
  private readonly config: Required<MemioConfig>;
  private _provider: SharedStateProvider | null = null;

  constructor(config: MemioConfig = {}) {
    this.config = {
      bufferName: config.bufferName ?? 'state',
      debug: config.debug ?? false,
    };
  }

  private get provider(): SharedStateProvider {
    if (!this._provider || (this._provider.platform() === 'unknown' && isSharedMemoryAvailable())) {
      this._provider = createSharedStateProvider();
    }
    return this._provider;
  }

  detectPlatform(): MemioPlatform {
    return detectPlatform();
  }

  isSharedMemoryAvailable(): boolean {
    return isSharedMemoryAvailable();
  }

  async waitForSharedMemory(name?: string, timeoutMs?: number): Promise<ArrayBuffer | Uint8Array | null> {
    const bufferName = name ?? this.config.bufferName;
    return waitForSharedBuffer(bufferName, timeoutMs);
  }

  readSharedState(lastVersion?: bigint): SharedStateSnapshot | null {
    return this.readSharedStateNamed(this.config.bufferName, lastVersion);
  }

  async readSharedStateAsync(lastVersion?: bigint): Promise<SharedStateSnapshot | null> {
    return this.readSharedStateNamedAsync(this.config.bufferName, lastVersion);
  }

  sharedManifest(): SharedStateManifest | null {
    return this.provider.sharedManifest();
  }

  readSharedStateNamed(name: string, lastVersion?: bigint): SharedStateSnapshot | null {
    const snapshot = this.provider.readSharedState(name, lastVersion);
    if (!snapshot) {
      this.log(`Shared buffer '${name}' not available`);
      return null;
    }
    return snapshot;
  }

  async readSharedStateNamedAsync(name: string, lastVersion?: bigint): Promise<SharedStateSnapshot | null> {
    if (this.provider.readSharedStateAsync) {
      const snapshot = await this.provider.readSharedStateAsync(name, lastVersion);
      if (!snapshot) {
        this.log(`Shared buffer '${name}' not available (async)`);
        return null;
      }
      return snapshot;
    }
    return this.readSharedStateNamed(name, lastVersion);
  }

  writeSharedState(data: ArrayBuffer | Uint8Array, version?: bigint): SharedStateWriteResult | null {
    return this.writeSharedStateNamed(this.config.bufferName, data, version);
  }

  writeSharedStateNamed(
    name: string,
    data: ArrayBuffer | Uint8Array,
    version?: bigint
  ): SharedStateWriteResult | null {
    const result = this.provider.writeSharedState(name, data, version);
    if (!result) {
      this.log(`Shared buffer '${name}' not available for write`);
      return null;
    }
    return result;
  }

  readSharedBuffer(name: string): Uint8Array | null {
    const buffer = this.provider.getSharedBuffer(name);
    if (!buffer) {
      return null;
    }
    return buffer instanceof Uint8Array ? buffer : new Uint8Array(buffer);
  }

  private log(message: string): void {
    if (this.config.debug) {
      console.log(`[MemioClient] ${message}`);
    }
  }
}

export function createMemioClient(config?: MemioConfig): MemioClient {
  return new MemioClient(config);
}
