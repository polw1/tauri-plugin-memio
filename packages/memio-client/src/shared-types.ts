import { StateView } from './state-view';

export interface SharedStateSnapshot {
  version: bigint;
  length: number;
  view: StateView;
}

export interface SharedStateWriteResult {
  version: bigint;
  length: number;
}

export type MemioPlatform = 'linux' | 'unknown';

export interface SharedStateManifest {
  version: number;
  buffers: Record<string, { length?: number }>;
}

export interface MemioGlobalBase {
  __memioSharedManifest?: SharedStateManifest;
  webkit?: {
    messageHandlers?: {
      memio?: { postMessage: (msg: unknown) => void };
    };
  };
}

export interface MemioLinuxGlobals extends MemioGlobalBase {
  memioSharedBuffer?: (name?: string) => ArrayBuffer | Uint8Array | null;
  memioWriteSharedBuffer?: (name: string, data: Uint8Array) => boolean;
  __memioSharedBuffers?: Record<string, ArrayBuffer | Uint8Array>;
  __memioSharedPath?: string;
  __memioSharedRegistryPath?: string;
}
