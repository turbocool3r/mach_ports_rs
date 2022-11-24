//! Provides definitions of types [`OolBuf`] and [`OolVec`] which should be used to handle OOL data
//! in Mach messages.

use std::mem::ManuallyDrop;
use std::{
    borrow::{Borrow, BorrowMut},
    error::Error,
    fmt,
    hash::{Hash, Hasher},
    mem,
    ops::{Deref, DerefMut},
    ptr::{self, NonNull},
    slice,
};

mod vm_buf {
    use mach2::{
        kern_return::*,
        traps, vm,
        vm_prot::{VM_PROT_READ, VM_PROT_WRITE},
        vm_statistics::VM_FLAGS_ANYWHERE,
    };
    use std::{
        ops::{Add, Rem, Sub},
        ptr::NonNull,
    };

    #[inline(always)]
    fn align_up<T: Copy + Add<Output = T> + Sub<Output = T> + Rem<Output = T>>(
        value: T,
        alignment: T,
    ) -> T {
        value + (alignment - value % alignment) % alignment
    }

    #[derive(Debug)]
    pub struct VmBuf {
        ptr: NonNull<u8>,
        capacity: usize,
    }

    impl VmBuf {
        /// Allocates a new `VmBuf` of the specified size.
        ///
        /// # Panics
        /// This function will panic in case:
        /// 1. The specified capacity is larger than `isize::MAX`.
        /// 2. A call to `mach_vm_allocate` returns an error.
        #[inline(always)]
        pub fn alloc(capacity: usize) -> Self {
            let ptr = if capacity > 0 {
                assert!(capacity <= (isize::MAX as usize));

                let mut address = 0;
                let size = capacity.try_into().unwrap();

                let result = unsafe {
                    vm::mach_vm_allocate(
                        traps::mach_task_self(),
                        &mut address,
                        size,
                        VM_PROT_READ | VM_PROT_WRITE | VM_FLAGS_ANYWHERE,
                    )
                };

                assert_eq!(result, KERN_SUCCESS);

                NonNull::new(address as *mut u8).unwrap()
            } else {
                NonNull::dangling()
            };

            Self { ptr, capacity }
        }

        /// Creates a new `VmBuf` from a pointer and a capacity value.
        #[inline]
        pub unsafe fn from_raw_parts(ptr: NonNull<u8>, capacity: usize) -> Self {
            Self { ptr, capacity }
        }

        /// Returns the address of the buffer represented by the `VmBuf`.
        #[inline]
        pub fn as_ptr(&self) -> NonNull<u8> {
            self.ptr
        }

        /// Returns the capacity of the buffer represented by the `VmBuf`.
        ///
        /// The actual capacity may be larger as it isn't rounded to the virtual memory page size.
        #[inline]
        pub fn capacity(&self) -> usize {
            self.capacity
        }

        pub fn shrink_to(&mut self, target_capacity: usize) {
            let cur_capacity = self.capacity;
            let page_size = page_size::get_granularity();
            let offset_in_page = self.ptr.addr().get() % page_size;

            assert!(target_capacity <= cur_capacity);
            assert!(page_size.is_power_of_two());

            let aligned_capacity =
                align_up(cur_capacity + offset_in_page, page_size) - offset_in_page;
            let aligned_target_capacity =
                align_up(target_capacity + offset_in_page, page_size) - offset_in_page;

            if aligned_target_capacity < aligned_capacity {
                let size = (cur_capacity - aligned_target_capacity).try_into().unwrap();

                // SAFETY: This is safe since aligned_target_capacity is less than  aligned_capacity
                // and aligned_capacity is the actual capacity of the buffer.
                let result = unsafe {
                    let address = self
                        .ptr
                        .as_ptr()
                        .addr()
                        .add(aligned_target_capacity)
                        .try_into()
                        .unwrap();

                    vm::mach_vm_deallocate(traps::mach_task_self(), address, size)
                };

                assert_eq!(result, KERN_SUCCESS);

                if aligned_target_capacity == 0 {
                    self.ptr = NonNull::dangling();
                }
            }

            self.capacity = target_capacity;
        }

        /// # Safety
        /// This function is safe to call if the underlying memory will not be accessed again. That
        /// means that either `self.ptr` must be set to `NonNull::dangling()` and `self.capacity` to
        /// 0 or the `VmBuf` should not be accessed by anything including the `Drop::drop`
        /// implementation.
        unsafe fn dealloc_impl(&mut self) -> kern_return_t {
            if self.capacity > 0 {
                let address = self.ptr.as_ptr().addr().try_into().unwrap();
                let size = self.capacity.try_into().unwrap();

                unsafe { vm::mach_vm_deallocate(traps::mach_task_self(), address, size) }
            } else {
                KERN_SUCCESS
            }
        }

        /// A panicking version of the Drop implementation.
        #[cfg(test)]
        pub fn dealloc(self) {
            let mut buf = std::mem::ManuallyDrop::new(self);

            // SAFETY: self is owned by the ManuallyDrop now and since drop won't be called on the
            // VmBuf nothing can access the dangling pointer.
            assert_eq!(unsafe { buf.dealloc_impl() }, KERN_SUCCESS);
        }
    }

    impl Default for VmBuf {
        fn default() -> Self {
            Self {
                ptr: NonNull::dangling(),
                capacity: 0,
            }
        }
    }

    impl Drop for VmBuf {
        #[inline]
        fn drop(&mut self) {
            // SAFETY: this is safe since after drop no one can access the fields of the VmBuf.
            unsafe {
                self.dealloc_impl();
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        #[should_panic]
        fn test_bad_dealloc() {
            let buf = VmBuf::alloc(page_size::get_granularity());
            let ptr = buf.as_ptr();

            buf.dealloc();

            let bad_buf = unsafe { VmBuf::from_raw_parts(ptr, usize::MAX) };
            bad_buf.dealloc();
        }
    }
}

use vm_buf::VmBuf;

/// A byte buffer backed by the Mach VM allocator.
///
/// It's intended to be used to represent out-of-line data buffers received in Mach messages.
#[derive(Default, Debug)]
pub struct OolBuf(VmBuf);

impl OolBuf {
    /// Constructs an [`OolBuf`] from a raw pointer and a length.
    ///
    /// # Safety
    /// The caller must ensure the pointer and the length represent a valid buffer allocated using
    /// the Mach VM API.
    pub unsafe fn from_raw_parts(ptr: NonNull<u8>, length: usize) -> Self {
        Self(VmBuf::from_raw_parts(ptr, length))
    }

    pub(crate) fn into_raw_parts(self) -> (NonNull<u8>, usize) {
        let buf = ManuallyDrop::new(self);
        (buf.0.as_ptr(), buf.0.capacity())
    }

    /// Returns a raw pointer to the buffer, or a dangling raw pointer valid for zero sized reads if
    /// the buffer's capacity is zero.
    pub fn as_ptr(&self) -> NonNull<u8> {
        self.0.as_ptr()
    }

    /// Extracts the slice with the contents of the buffer.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.0.as_ptr().as_ptr(), self.0.capacity()) }
    }

    /// Extracts a mutable slice with the contents of the buffer.
    #[inline]
    pub fn as_slice_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.0.as_ptr().as_ptr(), self.0.capacity()) }
    }

    /// Returns the length of the buffer in bytes.
    #[inline]
    pub fn len(&self) -> usize {
        self.0.capacity()
    }

    /// Returns `true` if the buffer is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Converts an [`OolBuf`] into an [`OolVec`].
    pub fn into_vec(self) -> OolVec {
        OolVec::from(self)
    }
}

impl From<OolVec> for OolBuf {
    fn from(mut value: OolVec) -> Self {
        value.shrink_to_fit();

        OolBuf(mem::take(&mut value.buf))
    }
}

impl PartialEq for OolBuf {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice().eq(other.as_slice())
    }
}

impl Eq for OolBuf {}

impl Hash for OolBuf {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl Borrow<[u8]> for OolBuf {
    #[inline(always)]
    fn borrow(&self) -> &[u8] {
        self.as_slice()
    }
}

impl BorrowMut<[u8]> for OolBuf {
    #[inline(always)]
    fn borrow_mut(&mut self) -> &mut [u8] {
        self.as_slice_mut()
    }
}

impl AsRef<[u8]> for OolBuf {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl AsMut<[u8]> for OolBuf {
    fn as_mut(&mut self) -> &mut [u8] {
        self.as_slice_mut()
    }
}

impl Deref for OolBuf {
    type Target = [u8];

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl DerefMut for OolBuf {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_slice_mut()
    }
}

/// An error returned when an [`OolVec`] doesn't have enough capacity.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
#[non_exhaustive]
pub struct NotEnoughCapacity {
    /// The capacity that was expected to be available by an extension operation.
    pub required_capacity: usize,
    /// The actual capacity currently available in the OOL buffer.
    pub available_capacity: usize,
}

impl fmt::Display for NotEnoughCapacity {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "could not extend OOL buffer with {} bytes as the available capacity is {}",
            self.required_capacity, self.available_capacity
        )
    }
}

impl Error for NotEnoughCapacity {}

/// A vector of bytes backed by the Mach VM allocator.
///
/// This is intended to be used to handle out-of-line data in Mach messages. More specifically, this
/// is a buffer that can be constructed in a more or less zero-cost
///
/// # Resizing the vector
/// Currently the capacity of the vector can only be decreased. Technically implementing growing
/// isn't very complicated, but the only use case for the capacity change I came up with is
/// truncating an overly large vector that is passed as an out-of-line buffer in a Mach message
/// with the deallocate flag set to `true`. This isn't even the best way to handle such a case since
/// freeing the buffer will require a call to `mach_vm_deallocate` and freeing a part using
/// `mach_msg` doesn't make a lot of sense.
#[derive(Default, Debug)]
pub struct OolVec {
    buf: VmBuf,
    len: usize,
}

impl OolVec {
    /// Allocates a new vector with the specified capacity.
    ///
    /// # Panics
    /// This function will panic in these cases:
    /// 1. The specified capacity is larger than [`isize::MAX`].
    /// 2. A call to `mach_vm_allocate` returns an error.
    #[inline(always)]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: VmBuf::alloc(capacity),
            len: 0,
        }
    }

    /// Creates an [`OolVec`] from a pointer, a length and a capacity.
    ///
    /// # Safety
    /// The caller must ensure the pointer points to a memory region allocated using the Mach VM
    /// API, the capacity matches the size of the region and the length is smaller than the
    /// capacity.
    pub unsafe fn from_raw_parts(ptr: NonNull<u8>, length: usize, capacity: usize) -> Self {
        Self {
            // SAFETY: safe since the safety requirements are satisfied by the caller.
            buf: unsafe { VmBuf::from_raw_parts(ptr, capacity) },
            len: length,
        }
    }

    /// Returns a raw pointer to the buffer, or a dangling raw pointer valid for zero sized reads if
    /// the buffer's capacity is zero.
    pub fn as_ptr(&self) -> NonNull<u8> {
        self.buf.as_ptr()
    }

    /// Extracts the slice with the contents of the buffer.
    ///
    /// # Example
    /// ```
    /// # use mach_ports::ool_vec;
    /// let mut buf = ool_vec![1, 2, 3];
    ///
    /// assert_eq!(buf.as_slice(), &[1, 2, 3]);
    /// ```
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.buf.as_ptr().as_ptr(), self.len) }
    }

    /// Extracts a mutable slice with the contents of the buffer.
    ///
    /// # Example
    /// ```
    /// # use mach_ports::ool_vec;
    /// let mut buf = ool_vec![1, 2, 3];
    ///
    /// buf.as_slice_mut()[1] = 4;
    ///
    /// assert_eq!(buf.as_slice(), &[1, 4, 3]);
    /// ```
    #[inline]
    pub fn as_slice_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.buf.as_ptr().as_ptr(), self.len) }
    }

    /// Returns the length of the buffer's contents in bytes.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns the capacity of the buffer in bytes.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.buf.capacity()
    }

    /// Returns `true` if the buffer is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Sets the new length of the buffer in bytes.
    ///
    /// # Safety
    /// The caller must ensure that the length is less than the buffer's capacity and that the
    /// contents of the buffer are not read until being initialized.
    #[inline]
    pub unsafe fn set_len(&mut self, new_len: usize) {
        assert!(new_len < self.capacity());

        self.len = new_len;
    }

    /// Tries to extend the vector with bytes from a byte slice.
    fn try_extend_from_slice(&mut self, slice: &[u8]) -> Result<(), NotEnoughCapacity> {
        let available_capacity = self.capacity() - self.len;

        if slice.len() <= available_capacity {
            unsafe {
                // SAFETY: This is safe since the length is always less than th capacity of the buffer,
                // the capacity is less than isize::MAX and since the buffer has been allocated the
                // address will not overflow.
                let dst = self.buf.as_ptr().as_ptr().add(self.len);

                // SAFETY: This is safe because the slice reference is immutable and the self
                // reference is mutable so there may not be any immutable references to the
                // underlying buffer. Additionally the memory location couldn't be referenced
                // because everything after the length can't be accessed without calling set_len.
                ptr::copy_nonoverlapping(slice.as_ptr(), dst, slice.len());

                // SAFETY: This is safe since we've just initialized slice.len() bytes. The addition
                // can not overflow since the result is verified to be less than the current
                // capacity.
                self.len += slice.len();
            };

            Ok(())
        } else {
            Err(NotEnoughCapacity {
                required_capacity: slice.len(),
                available_capacity,
            })
        }
    }

    fn try_push(&mut self, value: u8) -> Result<(), NotEnoughCapacity> {
        self.try_extend_from_slice(&[value])
    }

    /// Extends a vector with contents of a byte slice.
    ///
    /// # Example
    /// ```
    /// # use mach_ports::ool_vec;
    /// let mut v = ool_vec![1, 2, 3; 1024];
    ///
    /// v.extend_from_slice(&[4, 5, 6]);
    ///
    /// assert_eq!(v.as_slice(), &[1, 2, 3, 4, 5, 6])
    /// ```
    ///
    /// # Panics
    /// This function will panic in case the slice is longer than the available capacity.
    pub fn extend_from_slice(&mut self, slice: &[u8]) {
        self.try_extend_from_slice(slice).unwrap();
    }

    /// Pushes a byte to the end of the vector.
    ///
    /// # Panics
    /// This function will panic in case there is no available capacity in the vector.
    pub fn push(&mut self, value: u8) {
        self.try_push(value).unwrap();
    }

    /// Resizes the vector to a specified length.
    ///
    /// In case the new length is greater than the old one, the difference is filled with the
    /// specified value.
    ///
    /// # Errors
    /// Returns [`NotEnoughCapacity`] error in case the new length is greater than the vector's
    /// capacity.
    ///
    /// # Example
    /// ```
    /// # use mach_ports::ool_vec;
    /// let mut v = ool_vec![; 1024];
    ///
    /// v.resize(6, 0xAA).unwrap();
    /// v.resize(4, 0).unwrap();
    /// v.resize(5, 0xBB).unwrap();
    ///
    /// assert_eq!(v.as_slice(), &[0xAA, 0xAA, 0xAA, 0xAA, 0xBB]);
    /// ```
    pub fn resize(&mut self, new_len: usize, value: u8) -> Result<(), NotEnoughCapacity> {
        let available_capacity = self.capacity();

        if new_len <= available_capacity {
            let old_len = self.len;

            if old_len < new_len {
                // SAFETY: The bounds are verified to be safe above.
                unsafe {
                    let ptr = self.as_ptr().as_ptr().add(old_len);
                    let tail = slice::from_raw_parts_mut(ptr, new_len - old_len);

                    tail.fill(value);
                }
            }

            // SAFETY: new_len is definitely in bounds and the tail is initialized above.
            unsafe {
                self.set_len(new_len);
            }

            Ok(())
        } else {
            Err(NotEnoughCapacity {
                required_capacity: new_len,
                available_capacity,
            })
        }
    }

    /// Pops a byte from the vector in case it isn't empty.
    ///
    /// # Example
    /// ```
    /// # use mach_ports::ool_vec;
    /// let mut v = ool_vec![1, 2, 3];
    ///
    /// assert_eq!(v.pop(), Some(3));
    /// assert_eq!(v.pop(), Some(2));
    /// assert_eq!(v.pop(), Some(1));
    /// assert_eq!(v.pop(), None);
    /// ```
    pub fn pop(&mut self) -> Option<u8> {
        let (&value, _) = self.as_slice().split_last()?;

        // SAFETY: this is safe since as_slice() creates a slice using self.len as its length and
        // in case split_last() returns Some self.len wasn't zero.
        self.len -= 1;

        Some(value)
    }

    /// Shrinks the vector to the smallest capacity that can hold the stored data.
    pub fn shrink_to_fit(&mut self) {
        self.buf.shrink_to(self.len);
    }

    /// Converts an [`OolVec`] into an [`OolBuf`].
    pub fn into_buf(self) -> OolBuf {
        OolBuf::from(self)
    }
}

impl From<OolBuf> for OolVec {
    fn from(value: OolBuf) -> Self {
        let len = value.len();

        Self { buf: value.0, len }
    }
}

impl From<&'_ [u8]> for OolVec {
    fn from(value: &'_ [u8]) -> Self {
        let mut vec = OolVec::with_capacity(value.len());

        vec.extend_from_slice(value);

        vec
    }
}

impl PartialEq for OolVec {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice().eq(other.as_slice())
    }
}

impl Eq for OolVec {}

impl Hash for OolVec {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl Borrow<[u8]> for OolVec {
    #[inline(always)]
    fn borrow(&self) -> &[u8] {
        self.as_slice()
    }
}

impl BorrowMut<[u8]> for OolVec {
    #[inline(always)]
    fn borrow_mut(&mut self) -> &mut [u8] {
        self.as_slice_mut()
    }
}

impl AsRef<[u8]> for OolVec {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl AsMut<[u8]> for OolVec {
    fn as_mut(&mut self) -> &mut [u8] {
        self.as_slice_mut()
    }
}

impl Deref for OolVec {
    type Target = [u8];

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl DerefMut for OolVec {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_slice_mut()
    }
}

impl Extend<u8> for OolVec {
    fn extend<T: IntoIterator<Item = u8>>(&mut self, iter: T) {
        for value in iter {
            self.push(value);
        }
    }
}

/// Creates an [`OolVec`] from a list of elements and optionally a capacity value.
///
/// # Examples
/// ```
/// # use mach_ports::ool_vec;
/// // creating a vector from a list of elements
/// let v = ool_vec![1, 2, 3];
///
/// assert_eq!(v.as_slice(), &[1, 2, 3]);
/// assert_eq!(v.capacity(), 3);
///
/// // creating a vector from a list of elements and a capacity value
/// let v = ool_vec![1, 2, 3; 1024];
///
/// assert_eq!(v.as_slice(), &[1, 2, 3]);
/// assert_eq!(v.capacity(), 1024);
/// ```
#[macro_export]
macro_rules! ool_vec {
    (_count) => (0usize);
    (_count $x:tt $($xs:tt)*) => (1usize + $crate::ool_vec!(_count $($xs)*));
    ($($val:expr),*) => ({
        {
            let mut v = $crate::msg::ool::OolVec::with_capacity($crate::ool_vec!(_count $($val)*));

            $( v.push($val); )*

            v
        }
    });
    ($($val:expr),*; $cap:expr) => ({
        {
            let mut v = $crate::msg::ool::OolVec::with_capacity($cap);

            $( v.push($val); )*

            v
        }
    });
}
