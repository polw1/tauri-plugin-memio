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

export type MemioPlatform = 'linux' | 'android' | 'windows' | 'unknown';

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

export interface MemioAndroidGlobals extends MemioGlobalBase {
  /**
   * MemioNative is injected by MemioJsBridge.kt via WebView.addJavascriptInterface()
   *
   * NOTE: These methods are for metadata only. Actual data transfer uses:
   * - READ: memio:// protocol (MemioWebViewClient) - zero Base64
   * - WRITE: upload_file_from_uri (MemioPlugin) - zero Base64
   */
  MemioNative?: {
    /** Get version number from memio region */
    getVersion: (name: string) => number;
    /** List available memio buffers */
    listBuffers: () => string;
    /** Check if a buffer exists */
    hasBuffer: (name: string) => boolean;
    /** Write data into memio region (Base64 payload) */
    write?: (name: string, version: number, base64: string) => boolean;
    /** Get buffer metadata (JSON) */
    getBufferInfo: (name: string) => string | null;
    /** Debug information (JSON) */
    debug: () => string;
  };
  /** Version check */
  memioGetVersion?: (name?: string) => number;
  /** Flag indicating Android bridge is ready */
  __memioAndroidReady?: boolean;
}

export interface MemioLinuxGlobals extends MemioGlobalBase {
  memioSharedBuffer?: (name?: string) => ArrayBuffer | Uint8Array | null;
  memioWriteSharedBuffer?: (name: string, data: Uint8Array) => boolean;
  __memioSharedBuffers?: Record<string, ArrayBuffer | Uint8Array>;
  __memioSharedPath?: string;
  __memioSharedRegistryPath?: string;
}

export interface MemioWindowsGlobals extends MemioGlobalBase {
  /** Windows memio buffer access via Tauri IPC */
  memioWindowsSharedBuffer?: (name?: string) => ArrayBuffer | Uint8Array | null;
  memioWindowsWriteSharedBuffer?: (name: string, data: Uint8Array) => boolean;
  __memioWindowsSharedBuffers?: Record<string, ArrayBuffer | Uint8Array>;
  __memioWindowsReady?: boolean;
}
