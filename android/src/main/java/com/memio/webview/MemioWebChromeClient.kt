package com.memio.webview

import android.net.Uri
import android.util.Log
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebView

/**
 * Custom WebChromeClient that intercepts file picker to capture content:// URIs.
 * 
 * ## Problem:
 * JavaScript File API can't access Android content:// URIs directly due to scoped storage.
 * When user selects a file, JavaScript gets a File object but can't read it (NotReadableError).
 * 
 * ## Solution:
 * Intercept file picker, capture the content:// URI, expose it to JavaScript.
 * JavaScript sends the URI string (cheap!) to native code, which reads it via ContentResolver.
 * 
 * ## Architecture:
 * ```
 * User selects file
 *     ↓
 * onShowFileChooser() intercepts
 *     ↓
 * Captures content://... URI
 *     ↓
 * Injects JavaScript: window.__MEMIO_FILE_URI__
 *     ↓
 * JS sends URI to uploadFileFromUri command
 *     ↓
 * Kotlin reads via ContentResolver
 *     ↓
 * Writes directly to the memio region (no Base64)
 * ```
 * 
 * This is how real Android apps (Drive, Dropbox, etc) handle uploads efficiently.
 */
class MemioWebChromeClient(
    private val wrappedClient: WebChromeClient? = null
) : WebChromeClient() {
    
    companion object {
        private const val TAG = "MemioWebChromeClient"

        // Store last selected URIs so native plugin can fall back if invoke args are empty
        private var lastSelectedUris: List<Uri> = emptyList()

        fun getLastSelectedUri(): Uri? = lastSelectedUris.firstOrNull()
        fun getLastSelectedUris(): List<Uri> = lastSelectedUris
    }
    
    /**
     * Intercepts file picker to capture content:// URIs before they reach JavaScript.
     * 
     * When user selects file(s), we:
     * 1. Store the URI(s) 
     * 2. Inject JavaScript to expose URI to web code
     * 3. Pass files to original callback (for compatibility)
     */
    override fun onShowFileChooser(
        webView: WebView,
        filePathCallback: ValueCallback<Array<Uri>>,
        fileChooserParams: FileChooserParams
    ): Boolean {
        Log.d(TAG, "File picker opened")
        
        // Wrap callback to intercept selected files
        val wrappedCallback = ValueCallback<Array<Uri>> { uris ->
            if (uris != null && uris.isNotEmpty()) {
                Log.i(TAG, "Files selected: ${uris.size}")

                // Remember URIs for native fallback
                lastSelectedUris = uris.toList()
                
                // Expose URIs to JavaScript
                uris.forEachIndexed { index, uri ->
                    Log.d(TAG, "File $index: $uri")
                    
                    // Inject JavaScript to make URI accessible
                    val jsCode = """
                        (function() {
                            window.__MEMIO_FILE_URIS__ = window.__MEMIO_FILE_URIS__ || {};
                            window.__MEMIO_FILE_URIS__['file_$index'] = '$uri';
                            console.log('[MemioAndroid] File URI exposed: file_$index = $uri');
                        })();
                    """.trimIndent()
                    
                    webView.evaluateJavascript(jsCode) { result ->
                        Log.d(TAG, "JavaScript injection result: $result")
                    }
                }
            } else {
                Log.d(TAG, "File picker cancelled")
            }
            
            // Pass to original callback (for File object compatibility)
            filePathCallback.onReceiveValue(uris)
        }
        
        // Delegate to wrapped client or default implementation
        return wrappedClient?.onShowFileChooser(webView, wrappedCallback, fileChooserParams)
            ?: super.onShowFileChooser(webView, wrappedCallback, fileChooserParams)
    }
    
    // Delegate other methods to wrapped client
    
    override fun onProgressChanged(view: WebView?, newProgress: Int) {
        wrappedClient?.onProgressChanged(view, newProgress) 
            ?: super.onProgressChanged(view, newProgress)
    }
    
    override fun onReceivedTitle(view: WebView?, title: String?) {
        wrappedClient?.onReceivedTitle(view, title) 
            ?: super.onReceivedTitle(view, title)
    }
}
