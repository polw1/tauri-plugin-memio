//! Header read/write functions for memio regions.

pub use crate::shared_state_spec::{
    SHARED_STATE_ENDIANNESS, SHARED_STATE_HEADER_SIZE, SHARED_STATE_LENGTH_OFFSET,
    SHARED_STATE_MAGIC, SHARED_STATE_MAGIC_OFFSET, SHARED_STATE_VERSION_OFFSET,
};

use crate::{MemioError, MemioResult};

/// Returns true if buffer starts with valid magic bytes.
pub fn validate_magic(buf: &[u8]) -> bool {
    if buf.len() < SHARED_STATE_HEADER_SIZE {
        return false;
    }
    read_u64_le(buf, SHARED_STATE_MAGIC_OFFSET) == SHARED_STATE_MAGIC
}

/// Returns Ok if magic is valid, Err otherwise.
pub fn validate_magic_result(buf: &[u8]) -> MemioResult<()> {
    if buf.len() < SHARED_STATE_HEADER_SIZE {
        return Err(MemioError::Internal(
            "Shared state header too small.".to_string(),
        ));
    }
    let magic = read_u64_le(buf, SHARED_STATE_MAGIC_OFFSET);
    if magic != SHARED_STATE_MAGIC {
        return Err(MemioError::Internal(
            "Invalid shared state magic.".to_string(),
        ));
    }
    Ok(())
}

/// Writes magic, version, and length to buffer.
pub fn write_header(buf: &mut [u8], version: u64, length: usize) -> MemioResult<()> {
    if buf.len() < SHARED_STATE_HEADER_SIZE {
        return Err(MemioError::Internal(
            "Shared state header too small.".to_string(),
        ));
    }
    write_u64_le(buf, SHARED_STATE_MAGIC_OFFSET, SHARED_STATE_MAGIC);
    write_u64_le(buf, SHARED_STATE_VERSION_OFFSET, version);
    write_u64_le(buf, SHARED_STATE_LENGTH_OFFSET, length as u64);
    Ok(())
}

/// Writes header without Result. Returns false on failure.
pub fn write_header_unchecked(buf: &mut [u8], version: u64, length: usize) -> bool {
    if buf.len() < SHARED_STATE_HEADER_SIZE {
        return false;
    }
    write_u64_le(buf, SHARED_STATE_MAGIC_OFFSET, SHARED_STATE_MAGIC);
    write_u64_le(buf, SHARED_STATE_VERSION_OFFSET, version);
    write_u64_le(buf, SHARED_STATE_LENGTH_OFFSET, length as u64);
    true
}

/// Reads and validates header. Returns (version, length) if valid.
pub fn read_header(buf: &[u8], capacity: usize) -> Option<(u64, usize)> {
    if buf.len() < SHARED_STATE_HEADER_SIZE {
        return None;
    }
    if read_u64_le(buf, SHARED_STATE_MAGIC_OFFSET) != SHARED_STATE_MAGIC {
        return None;
    }
    let version = read_u64_le(buf, SHARED_STATE_VERSION_OFFSET);
    let length = read_u64_le(buf, SHARED_STATE_LENGTH_OFFSET) as usize;
    if length > capacity {
        return None;
    }
    Some((version, length))
}

/// Reads version from header.
pub fn read_version(buf: &[u8]) -> Option<u64> {
    if buf.len() < SHARED_STATE_HEADER_SIZE {
        return None;
    }
    Some(read_u64_le(buf, SHARED_STATE_VERSION_OFFSET))
}

/// Reads length from header.
pub fn read_length(buf: &[u8]) -> Option<usize> {
    if buf.len() < SHARED_STATE_HEADER_SIZE {
        return None;
    }
    Some(read_u64_le(buf, SHARED_STATE_LENGTH_OFFSET) as usize)
}

/// Reads header from raw pointer. Pointer must be valid for SHARED_STATE_HEADER_SIZE bytes.
/// # Safety
/// Caller must ensure `ptr` is valid for at least `capacity` bytes.
pub unsafe fn read_header_ptr(ptr: *const u8, capacity: usize) -> Option<(u64, usize)> {
    if ptr.is_null() {
        return None;
    }

    unsafe {
        let magic = read_u64_ptr(ptr, SHARED_STATE_MAGIC_OFFSET);
        if magic != SHARED_STATE_MAGIC {
            return None;
        }

        let version = read_u64_ptr(ptr, SHARED_STATE_VERSION_OFFSET);
        let length = read_u64_ptr(ptr, SHARED_STATE_LENGTH_OFFSET) as usize;

        if length > capacity {
            return None;
        }

        Some((version, length))
    }
}

/// Writes header to raw pointer. Pointer must be valid for SHARED_STATE_HEADER_SIZE bytes.
/// # Safety
/// Caller must ensure `ptr` is valid for at least `SHARED_STATE_HEADER_SIZE` bytes.
pub unsafe fn write_header_ptr(ptr: *mut u8, version: u64, length: usize) {
    unsafe {
        write_u64_ptr(ptr, SHARED_STATE_MAGIC_OFFSET, SHARED_STATE_MAGIC);
        write_u64_ptr(ptr, SHARED_STATE_VERSION_OFFSET, version);
        write_u64_ptr(ptr, SHARED_STATE_LENGTH_OFFSET, length as u64);
    }
}

/// Writes u64 in little-endian at offset.
#[inline]
pub fn write_u64_le(buf: &mut [u8], offset: usize, value: u64) {
    let bytes = value.to_le_bytes();
    buf[offset..offset + 8].copy_from_slice(&bytes);
}

/// Reads u64 in little-endian from offset.
#[inline]
pub fn read_u64_le(buf: &[u8], offset: usize) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&buf[offset..offset + 8]);
    u64::from_le_bytes(bytes)
}

/// Writes u64 in little-endian to pointer at offset.
#[inline]
/// # Safety
/// Caller must ensure `ptr` is valid for at least `offset + 8` bytes.
pub unsafe fn write_u64_ptr(ptr: *mut u8, offset: usize, value: u64) {
    let bytes = value.to_le_bytes();
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.add(offset), 8);
    }
}

/// Reads u64 in little-endian from pointer at offset.
#[inline]
/// # Safety
/// Caller must ensure `ptr` is valid for at least `offset + 8` bytes.
pub unsafe fn read_u64_ptr(ptr: *const u8, offset: usize) -> u64 {
    let mut bytes = [0u8; 8];
    unsafe {
        std::ptr::copy_nonoverlapping(ptr.add(offset), bytes.as_mut_ptr(), 8);
    }
    u64::from_le_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_roundtrip() {
        let mut buf = vec![0u8; SHARED_STATE_HEADER_SIZE + 100];
        assert!(write_header_unchecked(&mut buf, 42, 50));
        assert!(validate_magic(&buf));
        let (version, length) = read_header(&buf, 100).unwrap();
        assert_eq!(version, 42);
        assert_eq!(length, 50);
    }

    #[test]
    fn test_invalid_magic() {
        let buf = vec![0u8; SHARED_STATE_HEADER_SIZE];
        assert!(!validate_magic(&buf));
        assert!(read_header(&buf, 100).is_none());
    }

    #[test]
    fn test_read_version() {
        let mut buf = vec![0u8; SHARED_STATE_HEADER_SIZE];
        write_header_unchecked(&mut buf, 123, 0);
        assert_eq!(read_version(&buf), Some(123));
    }
}
