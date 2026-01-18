package com.memio.jsbridge

import android.webkit.JavascriptInterface
import android.webkit.WebView
import android.util.Base64
import com.memio.shared.MemioSharedMemory
import com.memio.spec.MemioSpec
import java.nio.ByteOrder

/**
 * Minimal JavaScript bridge for MemioTauri regions.
 *
 * Only provides fast version polling - actual data transfer uses:
 * - READ: memio:// protocol (MemioWebViewClient)
 * - WRITE: upload_file_from_uri (MemioPlugin)
 */
class MemioJsBridge(private val webView: WebView) {
    
    /**
     * Gets the version number from the memio region (fast check).
     * Used by frontend for polling changes without reading data.
     */
    @JavascriptInterface
    fun getVersion(name: String = "state"): Long {
        return try {
            val buffer = MemioSharedMemory.getDirectBuffer(name) ?: return -1
            buffer.order(ByteOrder.LITTLE_ENDIAN)
            buffer.position(MemioSpec.VERSION_OFFSET)
            buffer.getLong()
        } catch (e: Exception) {
            -1
        }
    }
    
    /**
     * Checks if a memio buffer exists.
     */
    @JavascriptInterface
    fun hasBuffer(name: String): Boolean {
        return MemioSharedMemory.exists(name)
    }

    /**
     * Legacy write using Base64 payloads from JS.
     *
     * Current clients should use upload_file_from_uri instead.
     */
    @JavascriptInterface
    fun write(name: String, version: Long, base64: String): Boolean {
        return try {
            val bytes = Base64.decode(base64, Base64.DEFAULT)
            MemioSharedMemory.write(name, version, bytes)
        } catch (e: Exception) {
            false
        }
    }
    
    companion object {
        const val BRIDGE_NAME = "MemioNative"
    }
    
    /**
     * Injects minimal JavaScript helpers into the WebView.
     */
    fun injectJsHelpers() {
        val js = """
            (function() {
                if (window.__memioAndroidReady) return;
                
                console.log('[MemioJsBridge] Injecting helpers (v3 - minimal)...');
                
                // Version check (fast, no data transfer)
                window.memioGetVersion = function(name) {
                    name = name || 'state';
                    try {
                        return window.MemioNative.getVersion(name);
                    } catch (e) {
                        return -1;
                    }
                };
                
                // Check buffer exists
                window.memioHasBuffer = function(name) {
                    try {
                        return window.MemioNative.hasBuffer(name);
                    } catch (e) {
                        return false;
                    }
                };

                // Backwards-compatible alias used by memio-client
                window.__TAURI_MEMIO__ = window.MemioNative;
                
                window.__memioAndroidReady = true;
                console.log('[MemioJsBridge] Ready (v3 - minimal)');
                window.dispatchEvent(new CustomEvent('memioReady', { detail: { platform: 'android' } }));
            })();
        """.trimIndent()
        
        webView.post {
            webView.evaluateJavascript(js, null)
        }
    }
}
