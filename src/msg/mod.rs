//! Contains data structures and functions that may be used to build and send/receive Mach messages.

pub mod buffer;
pub mod builder;
pub mod error;
pub mod ool;
pub mod parser;
#[cfg(test)]
mod tests;

pub use buffer::Buffer;
pub use builder::Builder;
pub use error::{RecvError, RecvErrorKind, SendError, SendErrorKind};
use mach2::{message::*, port::mach_port_right_t};
pub use parser::*;

/// A type for Mach message IDs.
pub type MsgId = mach_msg_id_t;

/// A wrapper for a mach_msg_bits_t value. Provides useful helper methods.
#[repr(transparent)]
#[derive(Default, Copy, Clone)]
struct MachMsgBits(mach_msg_bits_t);

impl MachMsgBits {
    pub const fn new(
        complex: bool,
        remote: mach_port_right_t,
        local: mach_port_right_t,
        voucher: mach_port_right_t,
    ) -> Self {
        assert!(remote == remote & MACH_MSGH_BITS_REMOTE_MASK);
        assert!(local == local & MACH_MSGH_BITS_REMOTE_MASK);
        assert!(voucher == voucher & MACH_MSGH_BITS_REMOTE_MASK);

        let port_bits = remote | (local << 8) | (voucher << 16);
        if complex {
            Self(port_bits | MACH_MSGH_BITS_COMPLEX)
        } else {
            Self(port_bits)
        }
    }

    #[inline(always)]
    pub const fn from_bits(value: mach_msg_bits_t) -> Self {
        assert!(value == (value & MACH_MSGH_BITS_USER));

        MachMsgBits(value)
    }

    #[inline(always)]
    pub const fn remote(self) -> mach_port_right_t {
        self.0 & MACH_MSGH_BITS_REMOTE_MASK
    }

    #[inline(always)]
    pub const fn set_remote(self, bits: mach_msg_bits_t) -> Self {
        assert!(bits == bits & MACH_MSGH_BITS_REMOTE_MASK);

        Self((self.0 & !MACH_MSGH_BITS_REMOTE_MASK) | bits)
    }

    #[inline(always)]
    pub const fn local(self) -> mach_port_right_t {
        (self.0 & MACH_MSGH_BITS_LOCAL_MASK) >> 8
    }

    #[inline(always)]
    pub const fn set_local(self, bits: mach_msg_bits_t) -> Self {
        assert!(bits == bits & MACH_MSGH_BITS_REMOTE_MASK);

        Self((self.0 & !MACH_MSGH_BITS_LOCAL_MASK) | (bits << 8))
    }

    #[inline(always)]
    pub const fn voucher(self) -> mach_port_right_t {
        (self.0 & MACH_MSGH_BITS_VOUCHER_MASK) >> 16
    }

    #[inline(always)]
    pub const fn complex(self) -> bool {
        (self.0 & MACH_MSGH_BITS_COMPLEX) == MACH_MSGH_BITS_COMPLEX
    }

    #[inline(always)]
    pub const fn into_complex(self) -> Self {
        Self(self.0 | MACH_MSGH_BITS_COMPLEX)
    }
}
