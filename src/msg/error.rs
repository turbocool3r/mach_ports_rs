//! Provides definitions of error types that may be returned when a Mach message exchange fails.
//!
//! Errors returned by the `mach_msg` API used to send/receive a message contain an error code and
//! 4 flags that may specify which subsystems reported an error during the Mach message transfer.
//! The error codes are split into two groups: "send" errors and "receive" errors. This module
//! provides two types that wrap these errors corresponding to the two groups: [`SendError`] and
//! [`RecvError`]. Both have identical APIs and their own error kind enumerations: [`SendErrorKind`]
//! and [`RecvErrorKind`].

use mach2::message::*;

macro_rules! def_error_kind {
    (
        $(#[$outer:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$inner:ident $($args:tt)*])*
                $var:ident = $val:ident,
            )+
        }
    ) => {
        $(#[$outer])*
        $vis enum $name {
            $(
                $(#[$inner $($args)*])*
                $var = $val as isize,
            )+
        }

        impl $name {
            #[doc = concat!(
                "Creates a `", stringify!($name), " from a known error code or returns `None`."
            )]
            pub const fn from_error_code(code: ::mach2::message::mach_msg_return_t) -> Option<Self> {
                match code {
                    $($val => Some(Self::$var),)+
                    _ => None,
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                let s = match self {
                    $(Self::$var => stringify!($val),)+
                };
                f.write_str(s)
            }
        }
    };
}

def_error_kind! {
    /// An error returned when sending a Mach message.
    pub enum SendErrorKind {
        /// Thread is waiting to send.  (Internal use only.)
        InProgress = MACH_SEND_IN_PROGRESS,
        /// Bogus in-line data.
        InvalidData = MACH_SEND_INVALID_DATA,
        /// Bogus destination port.
        InvalidDest = MACH_SEND_INVALID_DEST,
        /// Message not sent before timeout expired.
        TimedOut = MACH_SEND_TIMED_OUT,
        /// Bogus voucher port.
        InvalidVoucher = MACH_SEND_INVALID_VOUCHER,
        /// Software interrupt.
        Interrupted = MACH_SEND_INTERRUPTED,
        /// Data doesn't contain a complete message.
        MsgTooSmall = MACH_SEND_MSG_TOO_SMALL,
        /// Bogus reply port.
        InvalidReply = MACH_SEND_INVALID_REPLY,
        /// Bogus port rights in the message body.
        InvalidRight = MACH_SEND_INVALID_RIGHT,
        /// Bogus notify port argument.
        InvalidNotify = MACH_SEND_INVALID_NOTIFY,
        /// Invalid out-of-line memory pointer.
        InvalidMemory = MACH_SEND_INVALID_MEMORY,
        /// No message buffer is available.
        NoBuffer = MACH_SEND_NO_BUFFER,
        /// Send is too large for port
        TooLarge = MACH_SEND_TOO_LARGE,
        /// Invalid msg-type specification.
        InvalidType = MACH_SEND_INVALID_TYPE,
        /// A field in the header had a bad value.
        InvalidHeader = MACH_SEND_INVALID_HEADER,
        /// The trailer to be sent does not match kernel format.
        InvalidTrailer = MACH_SEND_INVALID_TRAILER,
        /// compatibility: no longer a returned error
        InvalidRtOolSize = MACH_SEND_INVALID_RT_OOL_SIZE,
    }
}

def_error_kind! {
    /// An error returned when receiving a Mach message.
    pub enum RecvErrorKind {
        /// Thread is waiting for receive.  (Internal use only.)
        InProgress = MACH_RCV_IN_PROGRESS,
        /// Bogus name for receive port/port-set.
        InvalidName = MACH_RCV_INVALID_NAME,
        /// Didn't get a message within the timeout value.
        TimedOut = MACH_RCV_TIMED_OUT,
        /// Message buffer is not large enough for inline data.
        TooLarge = MACH_RCV_TOO_LARGE,
        /// Software interrupt.
        Interrupted = MACH_RCV_INTERRUPTED,
        /// compatibility: no longer a returned error
        PortChanged = MACH_RCV_PORT_CHANGED,
        /// Bogus notify port argument.
        InvalidNotify = MACH_RCV_INVALID_NOTIFY,
        /// Bogus message buffer for inline data.
        InvalidData = MACH_RCV_INVALID_DATA,
        /// Port/set was sent away/died during receive.
        PortDied = MACH_RCV_PORT_DIED,
        /// compatibility: no longer a returned error
        InSet = MACH_RCV_IN_SET,
        /// Error receiving message header.  See special bits.
        HeaderError = MACH_RCV_HEADER_ERROR,
        /// Error receiving message body.  See special bits.
        BodyError = MACH_RCV_BODY_ERROR,
        /// Invalid msg-type specification in scatter list.
        InvalidType = MACH_RCV_INVALID_TYPE,
        /// Out-of-line overwrite region is not large enough
        ScatterSmall = MACH_RCV_SCATTER_SMALL,
        /// trailer type or number of trailer elements not supported
        InvalidTrailer = MACH_RCV_INVALID_TRAILER,
        /// Waiting for receive with timeout. (Internal use only.)
        InProgressTimed = MACH_RCV_IN_PROGRESS_TIMED,
    }
}

macro_rules! def_error {
    ($name:ident, $kind:ident, $doc:expr) => {
        #[repr(transparent)]
        #[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
        #[doc = $doc]
        pub struct $name(mach_msg_return_t);

        impl $name {
            /// Creates an error from its kind only.
            pub const fn from_kind(kind: $kind) -> Self {
                Self(kind as mach_msg_return_t)
            }

            /// Creates an error from its raw value.
            pub const fn from_bits(bits: mach_msg_return_t) -> Self {
                Self(bits)
            }

            /// Returns the error kind of the error.
            pub const fn kind(self) -> $kind {
                $kind::from_error_code(self.0 & !MACH_MSG_MASK).unwrap()
            }

            /// Returns the VM space flag of the error.
            #[inline(always)]
            pub const fn vm_space(self) -> bool {
                self.0 & MACH_MSG_VM_SPACE == MACH_MSG_VM_SPACE
            }

            /// Returns the same error with the VM space flag either set or reset.
            #[inline(always)]
            pub const fn set_vm_space(self, value: bool) -> Self {
                if value {
                    Self(self.0 | MACH_MSG_VM_SPACE)
                } else {
                    Self(self.0 & !MACH_MSG_VM_SPACE)
                }
            }

            /// Returns the VM kernel flag of the error.
            #[inline(always)]
            pub const fn vm_kernel(self) -> bool {
                self.0 & MACH_MSG_VM_KERNEL == MACH_MSG_VM_KERNEL
            }

            /// Returns the same error with the VM kernel flag either set or reset.
            #[inline(always)]
            pub const fn set_vm_kernel(self, value: bool) -> Self {
                if value {
                    Self(self.0 | MACH_MSG_VM_KERNEL)
                } else {
                    Self(self.0 & !MACH_MSG_VM_KERNEL)
                }
            }

            /// Returns the IPC space flag of the error.
            #[inline(always)]
            pub const fn ipc_space(self) -> bool {
                self.0 & MACH_MSG_IPC_SPACE == MACH_MSG_IPC_SPACE
            }

            /// Returns the same error with the IPC space flag either set or reset.
            #[inline(always)]
            pub const fn set_ipc_space(self, value: bool) -> Self {
                if value {
                    Self(self.0 | MACH_MSG_IPC_SPACE)
                } else {
                    Self(self.0 & !MACH_MSG_IPC_SPACE)
                }
            }

            /// Returns the IPC kernel flag of the error.
            #[inline(always)]
            pub const fn ipc_kernel(self) -> bool {
                self.0 & MACH_MSG_IPC_KERNEL == MACH_MSG_IPC_KERNEL
            }

            /// Returns the same error with the IPC kernel flag either set or reset.
            #[inline(always)]
            pub const fn set_ipc_kernel(self, value: bool) -> Self {
                if value {
                    Self(self.0 | MACH_MSG_IPC_KERNEL)
                } else {
                    Self(self.0 & !MACH_MSG_IPC_KERNEL)
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                ::std::fmt::Display::fmt(&self.kind(), f)?;

                if self.vm_space() {
                    f.write_str("|MACH_MSG_VM_SPACE")?;
                }

                if self.vm_kernel() {
                    f.write_str("|MACH_MSG_VM_KERNEL")?;
                }

                if self.ipc_space() {
                    f.write_str("|MACH_MSG_IPC_SPACE")?;
                }

                if self.ipc_kernel() {
                    f.write_str("|MACH_MSG_IPC_KERNEL")?;
                }

                Ok(())
            }
        }

        impl ::std::error::Error for $name {}
    };
}

def_error!(
    SendError,
    SendErrorKind,
    "Represents an error returned on message send failure."
);
def_error!(
    RecvError,
    RecvErrorKind,
    "Represents an error returned on message reception failure."
);
