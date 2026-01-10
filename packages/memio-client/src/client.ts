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

  /**
   * Returns the provider, creating it lazily.
   * This allows the client to detect shared memory that becomes available after construction.
   */
  private get provider(): SharedStateProvider {
    // Always recreate provider if current one is unknown but shared memory is now available
    if (!this._provider || (this._provider.platform() === 'unknown' && isSharedMemoryAvailable())) {
      this._provider = createSharedStateProvider();
    }
    return this._provider;
  }

  /**
   * Detects the current platform (linux/android/unknown).
   */
  detectPlatform(): MemioPlatform {
    return detectPlatform();
  }

  /**
   * Returns true if shared memory is available.
   */
  isSharedMemoryAvailable(): boolean {
    return isSharedMemoryAvailable();
  }

  /**
   * Waits for shared memory to become available.
   * For Windows, this also populates the provider cache.
   */
  async waitForSharedMemory(name?: string, timeoutMs?: number): Promise<ArrayBuffer | Uint8Array | null> {
    const bufferName = name ?? this.config.bufferName;
    const result = await waitForSharedBuffer(bufferName, timeoutMs);
    
    // For Windows, populate the provider cache
    if (result && this.provider.prepareCache) {
      await this.provider.prepareCache(bufferName);
    }
    
    return result;
  }

  /**
   * Reads the latest shared state (default buffer).
   * For Android, use readSharedStateAsync() instead.
   */
  readSharedState(lastVersion?: bigint): SharedStateSnapshot | null {
    return this.readSharedStateNamed(this.config.bufferName, lastVersion);
  }

  /**
   * Reads the latest shared state asynchronously.
   * Required for Android (memio:// protocol).
   */
  async readSharedStateAsync(lastVersion?: bigint): Promise<SharedStateSnapshot | null> {
    return this.readSharedStateNamedAsync(this.config.bufferName, lastVersion);
  }

  /**
   * Returns the shared manifest (if available).
   */
  sharedManifest(): SharedStateManifest | null {
    return this.provider.sharedManifest();
  }

  /**
   * Reads a named shared state buffer.
   * For Android, use readSharedStateNamedAsync() instead.
   */
  readSharedStateNamed(name: string, lastVersion?: bigint): SharedStateSnapshot | null {
    const snapshot = this.provider.readSharedState(name, lastVersion);
    if (!snapshot) {
      this.log(`Shared buffer '${name}' not available`);
      return null;
    }
    return snapshot;
  }

  /**
   * Reads a named shared state buffer asynchronously.
   * Required for Android (memio:// protocol).
   */
  async readSharedStateNamedAsync(name: string, lastVersion?: bigint): Promise<SharedStateSnapshot | null> {
    // Use async method if available (Android)
    if (this.provider.readSharedStateAsync) {
      const snapshot = await this.provider.readSharedStateAsync(name, lastVersion);
      if (!snapshot) {
        this.log(`Shared buffer '${name}' not available (async)`);
        return null;
      }
      return snapshot;
    }
    // Fall back to sync method (Linux, Windows)
    return this.readSharedStateNamed(name, lastVersion);
  }

  /**
   * Writes data to the default shared state buffer.
   */
  writeSharedState(data: ArrayBuffer | Uint8Array, version?: bigint): SharedStateWriteResult | null {
    return this.writeSharedStateNamed(this.config.bufferName, data, version);
  }

  /**
   * Writes data to a named shared state buffer.
   */
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

  /**
   * Reads a named shared buffer as raw bytes.
   */
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

/**
 * Creates a MemioClient instance.
 */
export function createMemioClient(config?: MemioConfig): MemioClient {
  return new MemioClient(config);
}
