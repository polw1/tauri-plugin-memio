package app.tauri.memio

import android.annotation.SuppressLint
import android.app.Activity
import android.webkit.WebView
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.memio.jsbridge.MemioJsBridge
import com.memio.shared.MemioSharedMemory
import com.memio.webview.MemioWebChromeClient
import com.memio.webview.MemioWebViewClient

/**
 * MemioPlugin - zero-copy shared memory bridge for Android WebView.
 *
 * Components used:
 * 1. JavaScriptInterface (MemioJsBridge) for commands/metadata
 * 2. memio:// protocol via MemioWebViewClient for downloads (raw bytes, sem Base64)
 * 3. Upload via ContentResolver (content:// URI) direto para MemioSharedMemory (sem Base64)
 *
 * O plugin encapsula WebViewClient e WebChromeClient para habilitar o protocolo e capturar file picker.
 */
@TauriPlugin
class MemioPlugin(private val activity: Activity) : Plugin(activity) {
    
    private var jsBridge: MemioJsBridge? = null
    private val contentResolver = activity.contentResolver
    
    @SuppressLint("SetJavaScriptEnabled")
    override fun load(webView: WebView) {
        android.util.Log.i("MemioPlugin", "MemioPlugin loaded - setting up memio:// protocol handler")
        
        // 1. Add JavaScript interface for commands
        jsBridge = MemioJsBridge(webView).also { bridge ->
            webView.addJavascriptInterface(bridge, MemioJsBridge.BRIDGE_NAME)
            android.util.Log.i("MemioPlugin", "Registered JavaScript bridge: ${MemioJsBridge.BRIDGE_NAME}")
        }
        
        // 2. Wrap WebViewClient to intercept memio:// URLs
        val currentClient = webView.webViewClient
        android.util.Log.i("MemioPlugin", "Current WebViewClient: ${currentClient?.javaClass?.name}")
        
        val memioClient = MemioWebViewClient(currentClient)
        webView.webViewClient = memioClient
        android.util.Log.i("MemioPlugin", "Installed MemioWebViewClient (wrapping ${currentClient?.javaClass?.simpleName})")
        
        // 3. Wrap WebChromeClient to intercept file picker
        val currentChromeClient = webView.webChromeClient
        android.util.Log.i("MemioPlugin", "Current WebChromeClient: ${currentChromeClient?.javaClass?.name}")
        
        val memioChromeClient = MemioWebChromeClient(currentChromeClient)
        webView.webChromeClient = memioChromeClient
        android.util.Log.i("MemioPlugin", "Installed MemioWebChromeClient for file picker interception")
        
        super.load(webView)
    }
    
    /**
     * Upload file from content:// URI to shared memory (ZERO BASE64!)
     * 
     * This is the optimal approach for Android uploads:
     * 1. JS sends only URI string (captured by MemioWebChromeClient)
     * 2. Kotlin reads file via ContentResolver
     * 3. Writes directly to shared memory
     * 
     * No Base64 encoding, no JavaScript file reads, true zero-copy!
     * 
     */
    @Command
    fun uploadFileFromUri(invoke: Invoke) {
        val data = invoke.parseArgs(JSObject::class.java)

        // Debug: log incoming keys/values to diagnose missing parameters
        runCatching {
            val names = data.names()
            val keys = mutableListOf<String>()
            if (names != null) {
                for (i in 0 until names.length()) {
                    keys.add(names.getString(i))
                }
            }
            android.util.Log.d("MemioPlugin", "uploadFileFromUri args keys: $keys")
            android.util.Log.d("MemioPlugin", "uploadFileFromUri raw data: $data")
        }

        // Accept both camelCase and snake_case keys; normalize empty -> default
        var bufferName = data.getString("bufferName")
            ?: data.getString("buffer_name")

        if (bufferName.isNullOrEmpty()) {
            bufferName = "upload"
        }

        var fileUri = data.getString("fileUri")
            ?: data.getString("file_uri")

        // Fallback: if invoke args are empty (observed on device), reuse last captured URI from WebChromeClient
        if (fileUri == null || fileUri.isEmpty()) {
            val fallback = com.memio.webview.MemioWebChromeClient.getLastSelectedUri()
            if (fallback != null) {
                fileUri = fallback.toString()
                android.util.Log.d("MemioPlugin", "Using fallback URI from WebChromeClient: $fileUri")
            }
        }

        android.util.Log.d("MemioPlugin", "uploadFileFromUri parsed bufferName='$bufferName', fileUri='$fileUri'")
        
        if (fileUri.isNullOrEmpty()) {
            invoke.reject("Missing fileUri parameter")
            return
        }
        
        try {
            val startTime = System.currentTimeMillis()
            val uri = android.net.Uri.parse(fileUri)
            
            android.util.Log.d("MemioPlugin", "Reading file from URI: $fileUri")
            
            // Read file via ContentResolver (proper Android way!)
            val inputStream = contentResolver.openInputStream(uri) ?: run {
                invoke.reject("Failed to open input stream for URI: $fileUri")
                return
            }
            
            // Read bytes (streaming, efficient)
            val data = inputStream.use { it.readBytes() }
            val readTime = System.currentTimeMillis() - startTime
            
            android.util.Log.d("MemioPlugin", "Read ${data.size} bytes in ${readTime}ms")
            
            // Auto-increment version
            val version = System.currentTimeMillis()
            
            // Write directly to shared memory (NO BASE64!)
            val writeStartTime = System.currentTimeMillis()
            val success = MemioSharedMemory.write(bufferName, version, data)
            val writeTime = System.currentTimeMillis() - writeStartTime
            
            if (!success) {
                invoke.reject("Failed to write to shared memory")
                return
            }
            
            val totalTime = System.currentTimeMillis() - startTime
            android.util.Log.i("MemioPlugin", "Native upload complete: ${data.size} bytes in ${totalTime}ms (read: ${readTime}ms, write: ${writeTime}ms)")
            
            // Return success with metadata
            val result = JSObject()
            result.put("success", true)
            result.put("bytesWritten", data.size)
            result.put("version", version)
            result.put("durationMs", totalTime.toDouble())
            result.put("readMs", readTime.toDouble())
            result.put("writeMs", writeTime.toDouble())
            
            invoke.resolve(result)
        } catch (e: Exception) {
            android.util.Log.e("MemioPlugin", "Upload failed", e)
            invoke.reject("Upload error: ${e.message}")
        }
    }
}
