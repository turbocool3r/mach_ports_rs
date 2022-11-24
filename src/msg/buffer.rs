//! Provides the [`Buffer`] structure used to represent a buffer for Mach messages.

use crate::msg::MachMsgBits;
use mach2::message::{mach_msg_header_t, mach_msg_size_t};
use std::{
    alloc::{self, Layout},
    cmp, mem,
    ptr::{self, NonNull},
    slice,
};

/// A helper structure that is used to represent unsized message contents.
#[repr(C)]
pub(crate) struct MsgData<T: ?Sized> {
    pub header: mach_msg_header_t,
    pub body: T,
}

/// A reusable buffer for Mach messages.
///
/// This structure isn't designed to be aware of the Mach message format and exists to allow reusing
/// memory when communicating using Mach messages.
#[derive(Debug)]
pub struct Buffer {
    ptr: NonNull<MsgData<[u8; 0]>>,
    len: mach_msg_size_t,
    capacity: mach_msg_size_t,
}

impl Buffer {
    fn layout_for_capacity(capacity: mach_msg_size_t) -> Layout {
        let (layout, _) = Layout::new::<mach_msg_header_t>()
            .extend(Layout::array::<u8>(capacity.try_into().unwrap()).unwrap())
            .unwrap();
        layout.pad_to_align()
    }

    /// Creates a new message buffer with the specified inline capacity. The capacity should not
    /// include the header's size as it is added automatically.
    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.try_into().unwrap();
        let layout = Self::layout_for_capacity(capacity);
        let ptr = unsafe {
            let ptr = NonNull::new(alloc::alloc(layout) as *mut MsgData<[u8; 0]>).unwrap();

            // Zero-out the header since technically it can be read at that point and this is an
            // UB.
            (*ptr.as_ptr()).header = Default::default();

            ptr
        };

        Self {
            ptr,
            len: 0,
            capacity,
        }
    }

    /// Returns the capacity of the inline data that fits into the buffer.
    pub fn capacity(&self) -> usize {
        self.capacity as usize
    }

    fn data(&self) -> &MsgData<[u8]> {
        let len = self.len as usize;
        let data = self.ptr.as_ptr() as *const u8;
        unsafe { &*(ptr::slice_from_raw_parts(data, len) as *const MsgData<[u8]>) }
    }

    fn data_mut(&mut self) -> &mut MsgData<[u8]> {
        let len = self.len as usize;
        let data = self.ptr.as_ptr() as *mut u8;
        unsafe { &mut *(ptr::slice_from_raw_parts_mut(data, len) as *mut MsgData<[u8]>) }
    }

    pub(crate) fn header(&self) -> &mach_msg_header_t {
        &self.data().header
    }

    pub(crate) fn header_mut(&mut self) -> &mut mach_msg_header_t {
        &mut self.data_mut().header
    }

    pub(crate) fn body(&self) -> &[u8] {
        &self.data().body
    }

    pub(crate) fn body_mut(&mut self) -> &mut [u8] {
        &mut self.data_mut().body
    }

    #[inline(always)]
    pub(super) fn header_bits(&self) -> MachMsgBits {
        MachMsgBits::from_bits(self.header().msgh_bits)
    }

    pub(crate) fn descriptors_count(&self) -> mach_msg_size_t {
        if self.header_bits().complex() {
            const SIZE_SIZE: usize = mem::size_of::<mach_msg_size_t>();

            let bytes: &[u8; SIZE_SIZE] = (&self.body()[..SIZE_SIZE]).try_into().unwrap();
            mach_msg_size_t::from_ne_bytes(*bytes)
        } else {
            0
        }
    }

    /// Returns the contents of the buffer as a byte slice.
    pub fn as_slice(&self) -> &[u8] {
        let len = self.body().len() + mem::size_of::<mach_msg_header_t>();
        let data = self.ptr.as_ptr() as *const u8;
        unsafe { slice::from_raw_parts(data, len) }
    }

    /// Reserves memory for the specified amount of additional bytes.
    pub(crate) fn reserve(&mut self, additional: mach_msg_size_t) {
        let requested_capacity = self.len.checked_add(additional).unwrap();
        let old_capacity = self.capacity;

        if requested_capacity > old_capacity {
            let new_capacity = cmp::max(old_capacity / 2, additional)
                .checked_add(old_capacity)
                .unwrap();
            let old_layout = Self::layout_for_capacity(old_capacity);
            let new_layout = Self::layout_for_capacity(new_capacity);

            let new_ptr = NonNull::new(unsafe {
                alloc::realloc(self.ptr.as_ptr() as *mut u8, old_layout, new_layout.size())
            } as *mut MsgData<[u8; 0]>)
            .unwrap();

            self.ptr = new_ptr;
            self.capacity = new_capacity;
        }
    }

    /// Appends bytes at the end of the buffer.
    pub(crate) fn append(&mut self, bytes: &[u8]) {
        let appended_len: mach_msg_size_t = bytes.len().try_into().unwrap();
        let space_left = self.capacity - self.len;
        if space_left < appended_len {
            self.reserve(appended_len - space_left);
        }

        // SAFETY: The buffer must have been allocated by that point. Since before the call the
        // destination part of the buffer wasn't publicly accessible, the source and the
        // destination should never overlap.
        let len = self.len as usize;
        let ptr = self.body_mut()[len..].as_mut_ptr();
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        }

        self.len += appended_len;
    }

    /// Inserts bytes at the specified offset into the inline part of the buffer (that means the
    /// offset is calculated from the end of the header).
    pub(crate) fn insert(&mut self, at: mach_msg_size_t, bytes: &[u8]) {
        assert!(at <= self.len);

        let inserted_len: mach_msg_size_t = bytes.len().try_into().unwrap();
        let space_left = self.capacity - self.len;
        let final_len = inserted_len.checked_add(at).unwrap();
        if space_left < final_len {
            self.reserve(final_len - space_left);
        }

        let body_ptr = self.body_mut().as_mut_ptr();
        let dst_ptr = unsafe { body_ptr.add(at as usize) };

        let moved_data_len = (self.len - at) as usize;
        if moved_data_len > 0 {
            let moved_data_off = (at + inserted_len) as usize;

            unsafe {
                ptr::copy(dst_ptr, body_ptr.add(moved_data_off), moved_data_len);
            }
        }

        // SAFETY: The buffer is big enough. The source slice may never overlap with the body
        // since we hold a mutable reference to the whole structure.
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), dst_ptr, bytes.len());
        }

        self.len += inserted_len;
    }

    /// Sets a new length for the buffer without performing any checks.
    pub(crate) unsafe fn set_len(&mut self, new_len: mach_msg_size_t) {
        assert!(new_len <= self.capacity);

        self.len = new_len;
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            alloc::dealloc(
                self.ptr.as_ptr() as *mut u8,
                Self::layout_for_capacity(self.capacity),
            );

            // just a small safety feature
            self.ptr = NonNull::dangling();
        }
    }
}
