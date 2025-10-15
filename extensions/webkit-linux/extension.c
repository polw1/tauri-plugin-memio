#include <webkit2/webkit-web-extension.h>
#include <jsc/jsc.h>
#include <glib.h>
#include <string.h>
#include <sys/types.h>
#include <sys/stat.h>
#include <sys/mman.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>

#include "memio_spec.h"

// JavaScript helpers injected into WebView context
// NOTE: Keep synchronized with shared/memio-helpers.js
static const char* JS_MEMIO_SHARED_BUFFER = 
  "globalThis.memioSharedBuffer = function(name){ "
  "name = name || 'state'; "
  "return globalThis.__memioSharedBuffers ? globalThis.__memioSharedBuffers[name] : null; "
  "};";

static const char* JS_MEMIO_LIST_BUFFERS = 
  "globalThis.memioListBuffers = function(){ "
  "return globalThis.__memioSharedBuffers ? Object.keys(globalThis.__memioSharedBuffers) : []; "
  "};";

static const char* JS_MEMIO_SHARED_DEBUG = 
  "globalThis.__memioSharedDebug = function(){ "
  "return { "
    "has: !!globalThis.__memioSharedBuffers, "
    "keys: globalThis.__memioSharedBuffers ? Object.keys(globalThis.__memioSharedBuffers) : [] "
  "}; "
  "};";

typedef struct {
  char *path;
  GMappedFile *file;
  gsize file_len;
  guint64 last_version;
  guint64 last_length;
  gboolean failed;  // Track if mapping failed to avoid repeated logs
} SharedCache;

static GHashTable *shared_cache_map = NULL;
static gchar *registry_path = NULL;

static void shared_cache_free(gpointer data) {
  SharedCache *cache = (SharedCache *)data;
  if (!cache) {
    return;
  }
  if (cache->file) {
    g_mapped_file_unref(cache->file);
  }
  g_free(cache->path);
  g_free(cache);
}

static SharedCache *get_cache(const char *name) {
  if (!shared_cache_map) {
    shared_cache_map = g_hash_table_new_full(g_str_hash, g_str_equal, g_free, shared_cache_free);
  }
  SharedCache *cache = g_hash_table_lookup(shared_cache_map, name);
  if (!cache) {
    cache = g_new0(SharedCache, 1);
    g_hash_table_insert(shared_cache_map, g_strdup(name), cache);
  }
  return cache;
}

static gboolean ensure_cache(SharedCache *cache, const char *path) {
  if (!path || path[0] == '\0') {
    return FALSE;
  }

  if (!cache->path || strcmp(cache->path, path) != 0) {
    if (cache->file) {
      g_mapped_file_unref(cache->file);
      cache->file = NULL;
    }
    g_free(cache->path);
    cache->path = g_strdup(path);
    cache->failed = FALSE;  // Reset failed flag for new path

    GError *error = NULL;
    cache->file = g_mapped_file_new(path, FALSE, &error);
    if (!cache->file) {
      if (error) {
        g_error_free(error);
      }
      cache->failed = TRUE;
      return FALSE;
    }
    cache->file_len = g_mapped_file_get_length(cache->file);
    cache->last_version = 0;
    cache->last_length = 0;
  }

  // If previously failed, try again (file might exist now)
  if (cache->failed && !cache->file) {
    GError *error = NULL;
    cache->file = g_mapped_file_new(path, FALSE, &error);
    if (!cache->file) {
      if (error) {
        g_error_free(error);
      }
      return FALSE;  // Still failed, but don't log again
    }
    cache->file_len = g_mapped_file_get_length(cache->file);
    cache->last_version = 0;
    cache->last_length = 0;
    cache->failed = FALSE;
  }

  return cache->file != NULL;
}

static gboolean update_buffer(JSCContext *context, const char *name, const char *path) {
  SharedCache *cache = get_cache(name);
  if (!ensure_cache(cache, path)) {
    return FALSE;
  }

  gpointer data = (gpointer)g_mapped_file_get_contents(cache->file);
  if (!data || cache->file_len < MEMIO_HEADER_SIZE) {
    return FALSE;
  }

  guint64 magic = 0;
  guint64 version = 0;
  guint64 length = 0;
  memcpy(&magic, data, 8);
  memcpy(&version, (guint8 *)data + 8, 8);
  memcpy(&length, (guint8 *)data + 16, 8);

  // Allow empty buffers, but reject invalid magic values.
  if (magic != 0 && magic != MEMIO_MAGIC) {
    return FALSE;
  }

  if (length > (guint64)(cache->file_len - MEMIO_HEADER_SIZE)) {
    length = cache->file_len - MEMIO_HEADER_SIZE;
  }

  // Ensure manifest exists and update buffer metadata
  JSCValue *manifest = jsc_context_get_value(context, "__memioSharedManifest");
  if (!manifest || !jsc_value_is_object(manifest)) {
    JSCValue *init = jsc_context_evaluate(
        context,
        "globalThis.__memioSharedManifest = { version: 1, buffers: {} };",
        -1);
    if (init) {
      g_object_unref(init);
    }
    manifest = jsc_context_get_value(context, "__memioSharedManifest");
  }
  if (manifest && jsc_value_is_object(manifest)) {
    JSCValue *buffers = jsc_value_object_get_property(manifest, "buffers");
    if (!buffers || !jsc_value_is_object(buffers)) {
      if (buffers) {
        g_object_unref(buffers);
      }
      buffers = jsc_context_evaluate(context, "({})", -1);
      jsc_value_object_set_property(manifest, "buffers", buffers);
    }
    JSCValue *entry = jsc_context_evaluate(context, "({})", -1);
    JSCValue *length_val = jsc_value_new_number(context, (double)length);
    jsc_value_object_set_property(entry, "length", length_val);
    jsc_value_object_set_property(buffers, name, entry);
    if (length_val) g_object_unref(length_val);
    if (entry) g_object_unref(entry);
    if (buffers) g_object_unref(buffers);
  }

  // Always ensure __memioSharedBuffers exists in the current context
  JSCValue *shared = jsc_context_get_value(context, "__memioSharedBuffers");
  gboolean need_create = !shared || !jsc_value_is_object(shared);
  
  if (need_create) {
    JSCValue *result = jsc_context_evaluate(context, "globalThis.__memioSharedBuffers = {};", -1);
    if (result) {
      g_object_unref(result);
    }
    shared = jsc_context_get_value(context, "__memioSharedBuffers");
  }

  // Buffer not ready yet (empty) - don't fail, just skip for now
  if (magic == 0 || length == 0) {
    return TRUE;  // Return TRUE = file mapped ok, but no data yet (will retry)
  }

  gsize total = MEMIO_HEADER_SIZE + (gsize)length;
  if (total > cache->file_len) {
    total = cache->file_len;
  }

  // Check if we already have this buffer in THIS context with same version
  if (!need_create && version == cache->last_version && length == cache->last_length) {
    JSCValue *existing = jsc_value_object_get_property(shared, name);
    if (existing && jsc_value_is_typed_array(existing)) {
      // Buffer exists and version matches, just update the data
      gsize out_len = 0;
      gpointer out = jsc_value_typed_array_get_data(existing, &out_len);
      if (out && out_len >= total) {
        memcpy(out, data, total);
        return TRUE;
      }
    }
  }

  // Create new typed array and copy data
  JSCValue *typed = jsc_value_new_typed_array(context, JSC_TYPED_ARRAY_UINT8, total);
  gsize out_len = 0;
  gpointer out = jsc_value_typed_array_get_data(typed, &out_len);
  if (!out || out_len < total) {
    if (typed) g_object_unref(typed);
    return FALSE;
  }

  memcpy(out, data, total);
  jsc_value_object_set_property(shared, name, typed);
  cache->last_version = version;
  cache->last_length = length;
  g_message("memio-webkit-extension: set __memioSharedBuffers[%s] len=%zu", name, total);
  return TRUE;
}

static gboolean load_registry(JSCContext *context) {
  const char *path = g_getenv("MEMIO_SHARED_REGISTRY");
  gchar *owned = NULL;
  
  // Log environment for debugging
  static gboolean env_logged = FALSE;
  if (!env_logged) {
    g_message("memio-webkit-extension: MEMIO_SHARED_REGISTRY=%s", path ? path : "(null)");
    const char *shared_path = g_getenv("MEMIO_SHARED_PATH");
    g_message("memio-webkit-extension: MEMIO_SHARED_PATH=%s", shared_path ? shared_path : "(null)");
    env_logged = TRUE;
  }
  
  if ((!path || path[0] == '\0') && context) {
    JSCValue *val = jsc_context_get_value(context, "__memioSharedRegistryPath");
    if (val && jsc_value_is_string(val)) {
      owned = jsc_value_to_string(val);
      path = owned;
    }
  }
  if (path && path[0] != '\0') {
    if (!registry_path || strcmp(registry_path, path) != 0) {
      g_free(registry_path);
      registry_path = g_strdup(path);
    }

    gchar *contents = NULL;
    gsize length = 0;
    if (!g_file_get_contents(path, &contents, &length, NULL)) {
      g_message("memio-webkit-extension: failed to read registry file %s", path);
      g_free(owned);
      return FALSE;
    }

    gchar **lines = g_strsplit(contents, "\n", -1);
    for (gchar **line = lines; line && *line; line++) {
      gchar *trimmed = g_strstrip(*line);
      if (trimmed[0] == '\0') {
        continue;
      }
      gchar **parts = g_strsplit(trimmed, "=", 2);
      if (parts[0] && parts[1]) {
        gchar *name = g_strstrip(parts[0]);
        gchar *buf_path = g_strstrip(parts[1]);
        if (name[0] != '\0' && buf_path[0] != '\0') {
          SharedCache *cache = get_cache(name);
          if (!update_buffer(context, name, buf_path)) {
            // Only log first failure for each buffer
            if (!cache->failed) {
              g_message("memio-webkit-extension: failed to map %s=%s", name, buf_path);
              cache->failed = TRUE;
            }
          }
        }
      }
      g_strfreev(parts);
    }
    g_strfreev(lines);
    g_free(contents);
    g_free(owned);
    return TRUE;
  }

  if (context) {
    JSCValue *val = jsc_context_get_value(context, "__memioSharedPath");
    if (val && jsc_value_is_string(val)) {
      gchar *direct_path = jsc_value_to_string(val);
      if (direct_path && direct_path[0] != '\0') {
        if (!update_buffer(context, "state", direct_path)) {
          g_message("memio-webkit-extension: failed to map direct state path %s", direct_path);
        }
      }
      g_free(direct_path);
      g_free(owned);
      return TRUE;
    }
  }

  g_free(owned);
  return FALSE;
}

static gboolean install_memio_bindings(gpointer user_data) {
  WebKitWebPage *page = WEBKIT_WEB_PAGE(user_data);
  WebKitFrame *frame = webkit_web_page_get_main_frame(page);
  if (!frame) {
    return G_SOURCE_REMOVE;
  }

  JSCContext *context = webkit_frame_get_js_context_for_script_world(
      frame, webkit_script_world_get_default());
  if (!context) {
    return G_SOURCE_REMOVE;
  }

  load_registry(context);

  // Inject core helpers (with guards to prevent re-injection)
  gchar *js1 = g_strdup_printf("if (!globalThis.memioSharedBuffer) { %s }", JS_MEMIO_SHARED_BUFFER);
  JSCValue *r1 = jsc_context_evaluate(context, js1, -1);
  g_free(js1);
  if (r1) g_object_unref(r1);
  
  gchar *js2 = g_strdup_printf("if (!globalThis.memioListBuffers) { %s }", JS_MEMIO_LIST_BUFFERS);
  JSCValue *r2 = jsc_context_evaluate(context, js2, -1);
  g_free(js2);
  if (r2) g_object_unref(r2);
  
  JSCValue *r3 = jsc_context_evaluate(context, JS_MEMIO_SHARED_DEBUG, -1);
  if (r3) g_object_unref(r3);

  g_message("memio-webkit-extension injected memioSharedBuffer");
  return G_SOURCE_REMOVE;
}

static gboolean refresh_shared_buffers(gpointer user_data) {
  WebKitWebPage *page = WEBKIT_WEB_PAGE(user_data);
  WebKitFrame *frame = webkit_web_page_get_main_frame(page);
  if (!frame) {
    return G_SOURCE_CONTINUE;
  }

  JSCContext *context = webkit_frame_get_js_context_for_script_world(
      frame, webkit_script_world_get_default());
  if (!context) {
    return G_SOURCE_CONTINUE;
  }

  load_registry(context);

  return G_SOURCE_CONTINUE;
}

// JavaScript callback: memioWriteSharedBuffer(name, uint8Array)
// Writes data from JavaScript directly to shared memory (no caching for simplicity)
static JSCValue *js_write_shared_buffer(GPtrArray *args) {
  if (args->len < 2) {
    g_warning("memioWriteSharedBuffer requires 2 arguments: name and data");
    return jsc_value_new_boolean(jsc_context_get_current(), FALSE);
  }

  JSCValue *name_val = g_ptr_array_index(args, 0);
  JSCValue *data_val = g_ptr_array_index(args, 1);

  if (!jsc_value_is_string(name_val)) {
    g_warning("memioWriteSharedBuffer: first argument must be a string");
    return jsc_value_new_boolean(jsc_context_get_current(), FALSE);
  }

  char *name = jsc_value_to_string(name_val);
  if (!name || name[0] == '\0') {
    g_free(name);
    return jsc_value_new_boolean(jsc_context_get_current(), FALSE);
  }

  // Get Uint8Array data
  if (!jsc_value_is_typed_array(data_val)) {
    g_warning("memioWriteSharedBuffer: second argument must be a Uint8Array");
    g_free(name);
    return jsc_value_new_boolean(jsc_context_get_current(), FALSE);
  }

  gsize data_len = 0;
  gpointer data = jsc_value_typed_array_get_data(data_val, &data_len);
  if (!data || data_len == 0) {
    g_warning("memioWriteSharedBuffer: failed to get typed array data");
    g_free(name);
    return jsc_value_new_boolean(jsc_context_get_current(), FALSE);
  }

  // Find buffer path from registry
  if (!registry_path) {
    g_warning("memioWriteSharedBuffer: registry not loaded");
    g_free(name);
    return jsc_value_new_boolean(jsc_context_get_current(), FALSE);
  }

  gchar *contents = NULL;
  gsize length = 0;
  if (!g_file_get_contents(registry_path, &contents, &length, NULL)) {
    g_warning("memioWriteSharedBuffer: failed to read registry");
    g_free(name);
    return jsc_value_new_boolean(jsc_context_get_current(), FALSE);
  }

  // Parse registry to find the buffer path
  gchar **lines = g_strsplit(contents, "\n", -1);
  g_free(contents);

  gchar *buffer_path = NULL;
  for (int i = 0; lines[i]; i++) {
    if (strlen(lines[i]) == 0) continue;
    gchar **eq_parts = g_strsplit(lines[i], "=", 2);
    if (g_strv_length(eq_parts) == 2 && strcmp(eq_parts[0], name) == 0) {
      buffer_path = g_strdup(eq_parts[1]);
      g_strfreev(eq_parts);
      break;
    }
    g_strfreev(eq_parts);
  }
  g_strfreev(lines);

  if (!buffer_path) {
    g_warning("memioWriteSharedBuffer: buffer '%s' not found in registry", name);
    g_free(name);
    return jsc_value_new_boolean(jsc_context_get_current(), FALSE);
  }

  // Open file for writing
  int fd = open(buffer_path, O_RDWR);
  if (fd < 0) {
    g_warning("memioWriteSharedBuffer: failed to open '%s': %s", buffer_path, strerror(errno));
    g_free(buffer_path);
    g_free(name);
    return jsc_value_new_boolean(jsc_context_get_current(), FALSE);
  }

  // Get file size
  struct stat st;
  if (fstat(fd, &st) < 0) {
    g_warning("memioWriteSharedBuffer: failed to stat '%s': %s", buffer_path, strerror(errno));
    close(fd);
    g_free(buffer_path);
    g_free(name);
    return jsc_value_new_boolean(jsc_context_get_current(), FALSE);
  }
  gsize file_len = st.st_size;

  // mmap with MAP_SHARED
  guint8 *file_data = mmap(NULL, file_len, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
  if (file_data == MAP_FAILED) {
    g_warning("memioWriteSharedBuffer: mmap failed for '%s': %s", buffer_path, strerror(errno));
    close(fd);
    g_free(buffer_path);
    g_free(name);
    return jsc_value_new_boolean(jsc_context_get_current(), FALSE);
  }

  // Verify we have enough space
  if (file_len < MEMIO_HEADER_SIZE + data_len) {
    g_warning("memioWriteSharedBuffer: buffer too small (%zu) for data (%zu)", file_len, data_len);
    munmap(file_data, file_len);
    close(fd);
    g_free(buffer_path);
    g_free(name);
    return jsc_value_new_boolean(jsc_context_get_current(), FALSE);
  }

  // Read current version from header
  guint64 current_version = 0;
  memcpy(&current_version, file_data + MEMIO_VERSION_OFFSET, 8);

  // Write data AFTER header
  memcpy(file_data + MEMIO_HEADER_SIZE, data, data_len);

  // Update header: increment version and set length
  guint64 new_version = current_version + 1;
  guint64 new_length = data_len;
  memcpy(file_data + MEMIO_VERSION_OFFSET, &new_version, 8);
  memcpy(file_data + MEMIO_LENGTH_OFFSET, &new_length, 8);

  g_message("memioWriteSharedBuffer: wrote %zu bytes to '%s' (version %lu)", data_len, name, new_version);

  munmap(file_data, file_len);
  close(fd);
  g_free(buffer_path);
  g_free(name);

  return jsc_value_new_boolean(jsc_context_get_current(), TRUE);
}

static void on_window_object_cleared(WebKitScriptWorld *world,
                                     WebKitWebPage *page,
                                     WebKitFrame *frame,
                                     gpointer user_data) {
  JSCContext *context = webkit_frame_get_js_context_for_script_world(frame, world);
  if (!context) {
    return;
  }

  g_message("memio-webkit-extension: window object cleared, injecting bindings");
  load_registry(context);

  // Inject core helpers
  JSCValue *r1 = jsc_context_evaluate(context, JS_MEMIO_SHARED_BUFFER, -1);
  if (r1) g_object_unref(r1);
  
  JSCValue *r2 = jsc_context_evaluate(context, JS_MEMIO_LIST_BUFFERS, -1);
  if (r2) g_object_unref(r2);
  
  JSCValue *r3 = jsc_context_evaluate(context, JS_MEMIO_SHARED_DEBUG, -1);
  if (r3) g_object_unref(r3);

  // Expose write function to JavaScript
  JSCValue *global = jsc_context_get_global_object(context);
  JSCValue *write_func = jsc_value_new_function_variadic(context,
                                                          "memioWriteSharedBuffer",
                                                          G_CALLBACK(js_write_shared_buffer),
                                                          NULL,
                                                          NULL,
                                                          JSC_TYPE_VALUE);
  jsc_value_object_set_property(global, "memioWriteSharedBuffer", write_func);
  g_object_unref(write_func);
  g_object_unref(global);

  g_message("memio-webkit-extension: bindings injected via window-object-cleared");
}

static void page_created(WebKitWebExtension *extension,
                         WebKitWebPage *page,
                         gpointer user_data) {
  g_message("memio-webkit-extension loaded (v3)");
  
  // Connect to window-object-cleared signal on the default script world
  WebKitScriptWorld *world = webkit_script_world_get_default();
  g_signal_connect(world, "window-object-cleared", G_CALLBACK(on_window_object_cleared), NULL);
  
  // Also try immediate injection for pages already loaded
  g_idle_add_full(G_PRIORITY_DEFAULT, install_memio_bindings, g_object_ref(page), g_object_unref);
  g_timeout_add_full(G_PRIORITY_DEFAULT, 100, refresh_shared_buffers, g_object_ref(page), g_object_unref);
}

G_MODULE_EXPORT void webkit_web_extension_initialize(WebKitWebExtension *extension) {
  g_signal_connect(extension, "page-created", G_CALLBACK(page_created), NULL);
}
