//! JNI bindings for MemioSharedMemory (Android)
//!
//! Methods used:
//! - nativeWrite: Write data to memio region
//! - nativeGetVersion: Get version from memio region header (via direct buffer)
//! - nativeExists: Check if a memio region exists
//! - nativeListRegions: List all memio regions
//! - nativeGetDirectBuffer: Get DirectByteBuffer for direct access

use jni::objects::{JByteArray, JObject, JString, JValue};
use jni::sys::jlong;
use jni::JNIEnv;

use memio_platform::{get_shared_ptr, has_shared_region, list_shared_regions, write_to_shared};

/// Writes data to a named memio region
/// Returns true on success, false on error
#[no_mangle]
pub extern "system" fn Java_com_memio_shared_MemioSharedMemory_nativeWrite(
    mut env: JNIEnv,
    _class: JObject,
    name: JString,
    version: jlong,
    data: JByteArray,
) -> bool {
    let name_str = match env.get_string(&name) {
        Ok(s) => String::from(s),
        Err(_) => return false,
    };

    let data_vec = match env.convert_byte_array(&data) {
        Ok(d) => d,
        Err(_) => return false,
    };

    match write_to_shared(&name_str, version as u64, &data_vec) {
        Ok(()) => true,
        Err(e) => {
            let _ = env.throw_new("java/lang/RuntimeException", format!("{:?}", e));
            false
        }
    }
}

/// Checks if a memio region exists
#[no_mangle]
pub extern "system" fn Java_com_memio_shared_MemioSharedMemory_nativeExists(
    mut env: JNIEnv,
    _class: JObject,
    name: JString,
) -> bool {
    let name_str = match env.get_string(&name) {
        Ok(s) => String::from(s),
        Err(_) => return false,
    };

    has_shared_region(&name_str)
}

/// Lists all memio regions
#[no_mangle]
pub extern "system" fn Java_com_memio_shared_MemioSharedMemory_nativeListRegions<'a>(
    mut env: JNIEnv<'a>,
    _class: JObject<'a>,
) -> JObject<'a> {
    let regions = list_shared_regions();

    let list_class = match env.find_class("java/util/ArrayList") {
        Ok(c) => c,
        Err(_) => return JObject::null(),
    };

    let list = match env.new_object(list_class, "()V", &[]) {
        Ok(l) => l,
        Err(_) => return JObject::null(),
    };

    for name in regions {
        let jname = match env.new_string(&name) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let jname_obj = JObject::from(jname);
        let _ = env.call_method(
            &list,
            "add",
            "(Ljava/lang/Object;)Z",
            &[JValue::Object(&jname_obj)],
        );
    }

    list
}

/// Gets raw pointer and size for direct buffer access
/// Returns a DirectByteBuffer or null on error
#[no_mangle]
pub extern "system" fn Java_com_memio_shared_MemioSharedMemory_nativeGetDirectBuffer<'a>(
    mut env: JNIEnv<'a>,
    _class: JObject<'a>,
    name: JString<'a>,
) -> JObject<'a> {
    let name_str = match env.get_string(&name) {
        Ok(s) => String::from(s),
        Err(_) => return JObject::null(),
    };

    let (ptr, size) = match get_shared_ptr(&name_str) {
        Ok(p) => p,
        Err(_) => return JObject::null(),
    };

    // Create a DirectByteBuffer from the raw pointer
    let buffer = unsafe {
        match env.new_direct_byte_buffer(ptr as *mut u8, size) {
            Ok(b) => b,
            Err(_) => return JObject::null(),
        }
    };

    buffer.into()
}
