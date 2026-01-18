package com.memio.webview

import android.util.Log
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import com.memio.shared.MemioSharedMemory
import java.io.ByteArrayInputStream
import java.nio.ByteOrder

/**
 * Custom WebViewClient that intercepts memio:// protocol for direct buffer access.
 * 
 * ## Architecture: C++ Data Plane + JS Controller
 * 
 * This approach eliminates Base64 encoding/decoding overhead by using a custom URL scheme:
 * 
 * ```
 * JavaScript                  Kotlin                    Rust/Memio region
 *    |                          |                              |
 *    |-- invoke("upload_file_from_uri") --->|                   |
 *    |                          |--- write to memio region ---->|
 *    |<-------- OK -------------|                              |
 *    |                          |                              |
 *    |-- fetch("memio://...") ->|                              |
 *    |                          |<-- read DirectByteBuffer ----|
 *    |<-- raw bytes (no Base64)-|                              |
 * ```
 * 
 * ## Usage:
 * ```javascript
 * // JS triggers a native upload via URI
 * await invoke("upload_file_from_uri", { bufferName: "file_transfer", fileUri });
 *
 * // JS fetches via custom protocol (no Base64)
 * const response = await fetch("memio://buffer/file_transfer");
 * const arrayBuffer = await response.arrayBuffer();
 * ```
 * 
 * ## Integration:
 * This client wraps the existing Tauri RustWebViewClient to avoid conflicts.
 */
class MemioWebViewClient(
    private val wrappedClient: WebViewClient? = null
) : WebViewClient() {
    
    companion object {
        private const val TAG = "MemioWebViewClient"
        private const val VERSION_OFFSET = 8
        private const val LENGTH_OFFSET = 16
        private const val DATA_OFFSET = 64
    }
    
    /**
     * Intercepts memio:// URLs and serves data from the memio region.
     * Also handles POST requests to write data to the memio region.
     * Falls back to wrapped client for other URLs.
     */
    override fun shouldInterceptRequest(
        view: WebView,
        request: WebResourceRequest
    ): WebResourceResponse? {
        val uri = request.url
        
        // Intercept memio://buffer/<name>
        if (uri.scheme == "memio" && uri.host == "buffer") {
            val bufferName = uri.pathSegments.firstOrNull()
            if (bufferName != null) {
                // Handle POST (write to buffer)
                if (request.method == "POST") {
                    Log.d(TAG, "Intercepting POST to memio://buffer/$bufferName")
                    return handleMemioWrite(bufferName, request)
                }
                // Handle GET (read from buffer)
                else if (request.method == "GET") {
                    Log.d(TAG, "Intercepting GET from memio://buffer/$bufferName")
                    return serveMemioBuffer(bufferName)
                }
            } else {
                Log.w(TAG, "Invalid memio:// URL: $uri (missing buffer name)")
                return createErrorResponse(400, "Missing buffer name")
            }
        }
        
        // Delegate to wrapped client (Tauri's RustWebViewClient)
        return wrappedClient?.shouldInterceptRequest(view, request)
            ?: super.shouldInterceptRequest(view, request)
    }
    
    /**
     * Handles POST request to write data to the memio region.
     * Since WebResourceRequest doesn't provide POST body access in Android,
     * we use a different approach: data is passed via custom X-Memio-Data header.
     */
    private fun handleMemioWrite(name: String, request: WebResourceRequest): WebResourceResponse {
        try {
            // WebResourceRequest doesn't support body in Android WebView
            // Data must be passed via header or we need JavascriptInterface fallback
            Log.w(TAG, "POST to memio:// is not fully supported in Android WebView (no body access)")
            Log.w(TAG, "Falling back to JavascriptInterface for writes")
            return createErrorResponse(501, "POST not implemented - use JavascriptInterface")
            
            /* Alternative approach if we get body access in the future:
            val data = ... get bytes from somewhere ...
            Log.d(TAG, "Received ${data.length} bytes to write to buffer '$name'")
            
            // Auto-increment version
            val version = System.currentTimeMillis()
            
            val success = MemioSharedMemory.write(name, version, data)
            
            if (!success) {
                Log.e(TAG, "Failed to write to buffer '$name'")
                return createErrorResponse(500, "Failed to write to memio region")
            }
            
            Log.d(TAG, "Successfully wrote ${data.length} bytes to buffer '$name' (version=$version)")
            
            // Return success response with metadata
            val responseJson = """{"success":true,"version":$version,"length":${data.length}}"""
            val responseStream = ByteArrayInputStream(responseJson.toByteArray())
            val headers = mapOf(
                "Content-Type" to "application/json",
                "X-Memio-Version" to version.toString(),
                "X-Memio-Length" to data.length.toString(),
                "Access-Control-Allow-Origin" to "*"
            )
            
            return WebResourceResponse(
                "application/json",
                "UTF-8",
                200,
                "OK",
                headers,
                responseStream
            )
            */
        } catch (e: Exception) {
            Log.e(TAG, "Error handling POST to buffer '$name'", e)
            return createErrorResponse(500, "Error writing buffer: ${e.message}")
        }
    }
    
    /**
     * Serves a memio buffer via WebResourceResponse.
     * This reads from DirectByteBuffer and returns raw bytes.
     */
    private fun serveMemioBuffer(name: String): WebResourceResponse {
        try {
            val buffer = MemioSharedMemory.getDirectBuffer(name)
            if (buffer == null) {
                Log.w(TAG, "Buffer '$name' not found")
                return createErrorResponse(404, "Buffer not found: $name")
            }
            
            // Read header (version and length)
            buffer.order(ByteOrder.LITTLE_ENDIAN)
            buffer.position(VERSION_OFFSET)
            val version = buffer.getLong()
            buffer.position(LENGTH_OFFSET)
            val length = buffer.getLong().toInt()
            
            // Validate length
            if (length <= 0 || length > buffer.capacity() - DATA_OFFSET) {
                Log.w(TAG, "Invalid length in buffer '$name': $length (capacity=${buffer.capacity()})")
                return createErrorResponse(500, "Invalid buffer length")
            }
            
            Log.d(TAG, "Serving buffer '$name': version=$version, length=$length bytes")
            
            // Read data (ONE copy - unavoidable for InputStream)
            buffer.position(DATA_OFFSET)
            val data = ByteArray(length)
            buffer.get(data)
            
            // Create response with raw bytes (no Base64)
            val inputStream = ByteArrayInputStream(data)
            val headers = mapOf(
                "Content-Type" to "application/octet-stream",
                "Content-Length" to length.toString(),
                "X-Memio-Version" to version.toString(),
                "X-Memio-Length" to length.toString(),
                "Access-Control-Allow-Origin" to "*",
                "Cache-Control" to "no-cache, no-store, must-revalidate"
            )
            
            return WebResourceResponse(
                "application/octet-stream",
                null, // encoding (null = binary)
                200,
                "OK",
                headers,
                inputStream
            )
        } catch (e: Exception) {
            Log.e(TAG, "Error serving buffer '$name'", e)
            return createErrorResponse(500, "Error reading buffer: ${e.message}")
        }
    }
    
    /**
     * Creates an error response with JSON body.
     */
    private fun createErrorResponse(statusCode: Int, message: String): WebResourceResponse {
        val json = """{"error":"$message"}"""
        val inputStream = ByteArrayInputStream(json.toByteArray())
        val headers = mapOf(
            "Content-Type" to "application/json",
            "Access-Control-Allow-Origin" to "*"
        )
        
        return WebResourceResponse(
            "application/json",
            "UTF-8",
            statusCode,
            when (statusCode) {
                400 -> "Bad Request"
                404 -> "Not Found"
                500 -> "Internal Server Error"
                else -> "Error"
            },
            headers,
            inputStream
        )
    }
    
    // Delegate other WebViewClient methods to wrapped client
    
    override fun onPageFinished(view: WebView?, url: String?) {
        wrappedClient?.onPageFinished(view, url) ?: super.onPageFinished(view, url)
    }
    
    override fun shouldOverrideUrlLoading(view: WebView?, request: WebResourceRequest?): Boolean {
        return wrappedClient?.shouldOverrideUrlLoading(view, request) 
            ?: super.shouldOverrideUrlLoading(view, request)
    }
}
