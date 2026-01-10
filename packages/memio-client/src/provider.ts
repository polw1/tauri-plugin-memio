import {
  getAndroidSharedBuffer,
  hasAndroidBridge,
  readSharedStateAndroid,
  readSharedStateAndroidAsync,
  writeSharedStateAndroid,
} from './platform/android';
import { getLinuxSharedBuffer, hasLinuxSharedMemory } from './platform/linux';
import {
  hasWindowsSharedBuffer,
  initWindowsSharedBuffer,
  downloadViaSharedBuffer,
  uploadViaSharedBuffer,
} from './platform/windows';
import type { SharedStateSnapshot, SharedStateWriteResult, MemioPlatform } from './shared-types';
import type { SharedStateManifest } from './shared-types';
import { readSharedState, writeSharedStateBuffer } from './shared-state';
import { getSharedManifest } from './shared-state';
import { StateView } from './state-view';

export interface SharedStateProvider {
  platform(): MemioPlatform;
  isAvailable(): boolean;
  sharedManifest(): SharedStateManifest | null;
  getSharedBuffer(name?: string): ArrayBuffer | Uint8Array | null;
  readSharedState(name?: string, lastVersion?: bigint): SharedStateSnapshot | null;
  /** Async read - required for Android (memio:// protocol) */
  readSharedStateAsync?(name?: string, lastVersion?: bigint): Promise<SharedStateSnapshot | null>;
  writeSharedState(name: string, data: ArrayBuffer | Uint8Array, version?: bigint): SharedStateWriteResult | null;
  /** Pre-populate cache for Windows (async operation) */
  prepareCache?(name: string): Promise<boolean>;
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

class AndroidProvider implements SharedStateProvider {
  platform(): MemioPlatform {
    return 'android';
  }

  isAvailable(): boolean {
    return hasAndroidBridge();
  }

  sharedManifest(): SharedStateManifest | null {
    return getSharedManifest();
  }

  getSharedBuffer(name?: string): ArrayBuffer | Uint8Array | null {
    return getAndroidSharedBuffer(name);
  }

  /**
   * Synchronous read - returns null on Android.
   * Use readSharedStateAsync() instead.
   */
  readSharedState(name?: string, lastVersion?: bigint): SharedStateSnapshot | null {
    return readSharedStateAndroid(name, lastVersion);
  }

  /**
   * Async read using memio:// protocol (ZERO BASE64!)
   */
  async readSharedStateAsync(name?: string, lastVersion?: bigint): Promise<SharedStateSnapshot | null> {
    return readSharedStateAndroidAsync(name, lastVersion);
  }

  writeSharedState(name: string, data: ArrayBuffer | Uint8Array, version?: bigint): SharedStateWriteResult | null {
    return writeSharedStateAndroid(name, data, version);
  }
}

class WindowsProvider implements SharedStateProvider {
  // Cache for synchronous access - populated via async IPC calls
  private cache: Map<string, { version: bigint; data: Uint8Array }> = new Map();

  constructor() {
    // Initialize SharedBuffer listener immediately
    initWindowsSharedBuffer();
  }

  platform(): MemioPlatform {
    return 'windows';
  }

  isAvailable(): boolean {
    return hasWindowsSharedBuffer();
  }

  sharedManifest(): SharedStateManifest | null {
    return getSharedManifest();
  }

  getSharedBuffer(name?: string): ArrayBuffer | Uint8Array | null {
    const cached = this.cache.get(name ?? 'state');
    return cached?.data ?? null;
  }

  readSharedState(name?: string, lastVersion?: bigint): SharedStateSnapshot | null {
    const stateName = name ?? 'state';
    const cached = this.cache.get(stateName);
    
    if (!cached) {
      // Cache miss - trigger async refresh for next call
      this.refreshCacheAsync(stateName);
      console.debug('[MemioWindows] Cache miss for:', stateName, '- triggering refresh');
      return null;
    }
    
    // Check version - if same, return empty (no change)
    if (lastVersion !== undefined && cached.version === lastVersion) {
      return {
        version: cached.version,
        length: 0,
        view: new StateView(new Uint8Array(0)),
      };
    }
    
    return {
      version: cached.version,
      length: cached.data.length,
      view: new StateView(cached.data),
    };
  }

  writeSharedState(name: string, data: ArrayBuffer | Uint8Array, version?: bigint): SharedStateWriteResult | null {
    // Trigger async write
    this.writeAsync(name, data, version);
    
    const newVersion = version ?? BigInt(Date.now());
    const dataBytes = data instanceof ArrayBuffer ? new Uint8Array(data) : data;
    
    // Update local cache immediately
    this.cache.set(name, { version: newVersion, data: dataBytes });
    
    return {
      version: newVersion,
      length: dataBytes.length,
    };
  }
  
  private async refreshCacheAsync(name: string): Promise<boolean> {
    try {
      // Use SharedBuffer zero-copy download
      const result = await downloadViaSharedBuffer(name);
      
      if (result) {
        this.cache.set(name, {
          version: result.version,
          data: result.data,
        });
        console.debug('[MemioWindows] Cache refreshed for:', name, 'version:', result.version, 'length:', result.data.length);
        return true;
      }
      return false;
    } catch (error) {
      console.debug('[MemioWindows] Cache refresh failed:', error);
      return false;
    }
  }
  
  /** Pre-populate cache - call this before synchronous reads */
  async prepareCache(name: string): Promise<boolean> {
    return this.refreshCacheAsync(name);
  }
  
  private async writeAsync(name: string, data: ArrayBuffer | Uint8Array, version?: bigint): Promise<void> {
    try {
      const buffer = data instanceof ArrayBuffer ? new Uint8Array(data) : data;
      const ver = version !== undefined ? Number(version) : Date.now();
      // Use SharedBuffer zero-copy upload
      await uploadViaSharedBuffer(name, buffer, ver);
    } catch (error) {
      console.debug('[MemioWindows] Async write failed:', error);
    }
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
  if (hasWindowsSharedBuffer()) {
    console.debug('[MemioClient] Using Windows SharedBuffer provider');
    return new WindowsProvider();
  }
  if (hasAndroidBridge()) {
    return new AndroidProvider();
  }
  if (hasLinuxSharedMemory()) {
    return new LinuxProvider();
  }
  return new UnknownProvider();
}
