package com.memio.shared

import android.util.Log
import java.nio.ByteBuffer

/**
 * Kotlin wrapper for native shared memory operations.
 * 
 * Used by:
 * - MemioWebViewClient: getDirectBuffer() for memio:// reads
 * - MemioPlugin: write() for file uploads
 * - MemioJsBridge: getVersion(), exists(), listRegions() for metadata
 */
object MemioSharedMemory {
    
    private const val TAG = "MemioSharedMemory"
    
    // Cache of direct ByteBuffers - true zero-copy for Kotlin access
    private val directBuffers = mutableMapOf<String, ByteBuffer>()
    
    /**
     * Writes data to a named shared memory region.
     * Used by MemioPlugin.uploadFileFromUri()
     */
    fun write(name: String, version: Long, data: ByteArray): Boolean {
        return try {
            nativeWrite(name, version, data)
        } catch (e: Exception) {
            Log.e(TAG, "Failed to write to shared memory '$name': ${e.message}")
            false
        }
    }
    
    /**
     * Gets the current version from the shared memory header.
     * Used by MemioJsBridge.getVersion() for fast version polling
     */
    fun getVersion(name: String): Long {
        return try {
            nativeGetVersion(name)
        } catch (e: Exception) {
            Log.e(TAG, "Failed to get version for '$name': ${e.message}")
            -1
        }
    }
    
    /**
     * Checks if a shared memory region exists.
     * Used by MemioJsBridge.hasBuffer()
     */
    fun exists(name: String): Boolean {
        return try {
            nativeExists(name)
        } catch (e: Exception) {
            false
        }
    }
    
    /**
     * Lists all registered shared memory regions.
     * Used by MemioJsBridge.listBuffers()
     */
    fun listRegions(): List<String> {
        return try {
            @Suppress("UNCHECKED_CAST")
            (nativeListRegions() as? List<String>) ?: emptyList()
        } catch (e: Exception) {
            Log.e(TAG, "Failed to list regions: ${e.message}")
            emptyList()
        }
    }
    
    /**
     * Gets a direct ByteBuffer for TRUE zero-copy access to shared memory.
     * Used by MemioWebViewClient.serveMemioBuffer() for memio:// protocol
     * 
     * IMPORTANT: This buffer includes the 64-byte header:
     * - Bytes 0-7: Magic number (MEMIOSHR)
     * - Bytes 8-15: Version (little-endian u64)
     * - Bytes 16-23: Data length (little-endian u64)
     * - Bytes 24-63: Reserved
     * - Bytes 64+: Actual data
     */
    fun getDirectBuffer(name: String): ByteBuffer? {
        // Check cache first
        directBuffers[name]?.let { return it }
        
        return try {
            val buffer = nativeGetDirectBuffer(name) as? ByteBuffer
            if (buffer != null && buffer.isDirect) {
                directBuffers[name] = buffer
                Log.i(TAG, "Got direct buffer for '$name', capacity=${buffer.capacity()}")
            }
            buffer
        } catch (e: Exception) {
            Log.e(TAG, "Failed to get direct buffer for '$name': ${e.message}")
            null
        }
    }
    
    // Native method declarations (only what's used)
    private external fun nativeWrite(name: String, version: Long, data: ByteArray): Boolean
    private external fun nativeGetVersion(name: String): Long
    private external fun nativeExists(name: String): Boolean
    private external fun nativeListRegions(): Any
    private external fun nativeGetDirectBuffer(name: String): Any?
}


