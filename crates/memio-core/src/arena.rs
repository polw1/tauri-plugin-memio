//! Fixed-size memory arena.

use std::alloc::{Layout, alloc, dealloc};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Pre-allocated memory block with bump allocation.
pub struct Arena {
    base: NonNull<u8>,
    capacity: usize,
    offset: AtomicUsize,
}

unsafe impl Send for Arena {}
unsafe impl Sync for Arena {}

impl Arena {
    /// Allocates a new arena with the given capacity.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "Arena capacity must be greater than zero");

        let layout = Layout::from_size_align(capacity, 16).expect("Invalid layout for arena");

        let ptr = unsafe { alloc(layout) };
        let base = NonNull::new(ptr).expect("Failed to allocate arena memory");

        Self {
            base,
            capacity,
            offset: AtomicUsize::new(0),
        }
    }

    /// Allocates `size` bytes with alignment. Returns None if full.
    pub fn alloc(&self, size: usize, align: usize) -> Option<NonNull<u8>> {
        loop {
            let current = self.offset.load(Ordering::Relaxed);
            let aligned = (current + align - 1) & !(align - 1);
            let new_offset = aligned + size;

            if new_offset > self.capacity {
                return None;
            }

            if self
                .offset
                .compare_exchange_weak(current, new_offset, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                let ptr = unsafe { self.base.as_ptr().add(aligned) };
                return NonNull::new(ptr);
            }
        }
    }

    /// Resets offset to zero. Caller must ensure no references exist.
    ///
    /// # Safety
    /// The caller must ensure no live references point into the arena.
    pub unsafe fn reset(&self) {
        self.offset.store(0, Ordering::SeqCst);
    }

    /// Returns bytes allocated.
    pub fn used(&self) -> usize {
        self.offset.load(Ordering::Relaxed)
    }

    /// Returns total capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns base address pointer.
    pub fn as_ptr(&self) -> *const u8 {
        self.base.as_ptr()
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        let layout =
            Layout::from_size_align(self.capacity, 16).expect("Invalid layout during deallocation");

        unsafe {
            dealloc(self.base.as_ptr(), layout);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arena_allocation() {
        let arena = Arena::new(1024);

        let ptr1 = arena.alloc(100, 8).expect("First allocation failed");
        let ptr2 = arena.alloc(200, 8).expect("Second allocation failed");

        assert!(ptr1.as_ptr() != ptr2.as_ptr());
        assert!(arena.used() >= 300);
    }

    #[test]
    fn test_arena_full() {
        let arena = Arena::new(100);

        let _ = arena.alloc(50, 1);
        let _ = arena.alloc(50, 1);

        assert!(arena.alloc(10, 1).is_none());
    }

    #[test]
    fn test_arena_reset() {
        let arena = Arena::new(1024);

        let _ = arena.alloc(500, 8);
        assert!(arena.used() >= 500);

        unsafe { arena.reset() };
        assert_eq!(arena.used(), 0);
    }
}
