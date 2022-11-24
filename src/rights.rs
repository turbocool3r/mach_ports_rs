//! Provides wrappers for Mach port right names.
//!
//! The module provides 3 types [`SendRight`], [`SendOnceRight`] and [`RecvRight`] that are wrappers
//! for raw `mach_port_t` values (aka Mach port names).
//!
//! # Ownership
//!
//! Each of these values represent a single user reference on the corresponding right to a Mach
//! port. That means when a value is dropped, the task loses a reference to the Mach port right
//! represented by the wrapped name (through a call to `mach_port_mod_refs`). Additionally,
//! [`SendRight`] wrappers can be cloned which increases the number of references to the port's
//! send right.

use crate::{
    msg::{Buffer, Builder, MsgParser, RecvError, SendError},
    traits::*,
};
use mach2::{
    kern_return::*,
    mach_port,
    message::*,
    port::{
        mach_port_delta_t, mach_port_right_t, mach_port_t, MACH_PORT_NULL,
        MACH_PORT_RIGHT_DEAD_NAME, MACH_PORT_RIGHT_RECEIVE, MACH_PORT_RIGHT_SEND,
        MACH_PORT_RIGHT_SEND_ONCE,
    },
    traps,
};
use std::mem::ManuallyDrop;

fn mod_refs_wrapper(
    name: mach_port_t,
    right: mach_port_right_t,
    delta: mach_port_delta_t,
) -> kern_return_t {
    let mut result =
        unsafe { mach_port::mach_port_mod_refs(traps::mach_task_self(), name, right, delta) };

    // KERN_INVALID_RIGHT happens when a right becomes a dead name. Otherwise something is broken
    // and we panic.
    if result == KERN_INVALID_RIGHT {
        result = unsafe {
            mach_port::mach_port_mod_refs(
                traps::mach_task_self(),
                name,
                MACH_PORT_RIGHT_DEAD_NAME,
                delta,
            )
        };
    }

    result
}

fn send_impl(name: mach_port_t, msg: Builder, bits: mach_msg_bits_t) -> Result<(), SendError> {
    let mut msg = ManuallyDrop::new(msg);

    msg.set_raw_remote_port(name, bits);

    let data = msg.as_slice();
    let result = unsafe {
        mach_msg(
            data.as_ptr() as *mut mach_msg_header_t,
            MACH_SEND_MSG,
            data.len() as mach_msg_size_t,
            0,
            MACH_PORT_NULL,
            0,
            MACH_PORT_NULL,
        )
    };

    if result == KERN_SUCCESS {
        Ok(())
    } else {
        Err(SendError::from_bits(result))
    }
}

/// A wrapper for a Mach port name that holds a send right to a port.
#[repr(transparent)]
#[derive(Debug)]
pub struct SendRight(mach_port_t);

impl SendRight {
    /// Creates a `SendRight` wrapper from a raw Mach port name.
    #[inline(always)]
    pub fn from_raw_name(name: mach_port_t) -> Self {
        SendRight(name)
    }

    #[inline(always)]
    fn mod_refs(&self, delta: mach_port_delta_t) -> kern_return_t {
        mod_refs_wrapper(self.0, MACH_PORT_RIGHT_SEND, delta)
    }

    /// Sends a message built by a [`Builder`].
    ///
    /// This function is a safe wrapper around the `mach_msg` API.
    ///
    /// # Port right references
    /// This method consumes all moved port right references that the message holds no matter if the
    /// message transfer is successful or not.
    pub fn send(&self, msg: Builder) -> Result<(), SendError> {
        send_impl(self.0, msg, MACH_MSG_TYPE_COPY_SEND)
    }
}

impl Clone for SendRight {
    #[inline(always)]
    fn clone(&self) -> Self {
        assert_eq!(self.mod_refs(1), KERN_SUCCESS);

        SendRight(self.0)
    }

    #[inline(always)]
    fn clone_from(&mut self, source: &Self) {
        assert_eq!(self.mod_refs(1), KERN_SUCCESS);

        self.0 = source.0;
    }
}

impl Drop for SendRight {
    #[inline(always)]
    fn drop(&mut self) {
        self.mod_refs(-1);
    }
}

impl AsRawName for SendRight {
    type Base = SendRight;

    #[inline(always)]
    fn as_raw_name(&self) -> mach_port_t {
        self.0
    }
}

impl<'a> AsRawName for &'a SendRight {
    type Base = SendRight;

    #[inline(always)]
    fn as_raw_name(&self) -> mach_port_t {
        self.0
    }
}

impl IntoRawName for SendRight {
    #[inline(always)]
    fn into_raw_name(self) -> mach_port_t {
        ManuallyDrop::new(self).0
    }
}

impl BaseRight for SendRight {
    const MSG_TYPE: mach_port_right_t = MACH_MSG_TYPE_MOVE_SEND;
}

impl BaseSendRight for SendRight {}

/// A wrapper for a Mach port name that holds a send once right to a port.
#[repr(transparent)]
#[derive(Debug)]
pub struct SendOnceRight(mach_port_t);

impl SendOnceRight {
    /// Creates a `SendOnceRight` wrapper from a raw `mach_port_t`.
    #[inline(always)]
    pub fn from_raw_name(name: mach_port_t) -> Self {
        SendOnceRight(name)
    }

    #[inline(always)]
    fn mod_refs(&self, delta: mach_port_delta_t) -> kern_return_t {
        mod_refs_wrapper(self.0, MACH_PORT_RIGHT_SEND_ONCE, delta)
    }

    /// Sends a message built by a [`Builder`] and consumes the send once right.
    ///
    /// This function is a safe wrapper around the `mach_msg` API.
    ///
    /// # Port right references
    /// This method consumes all moved port right references that the message holds no matter if the
    /// message transfer is successful or not.
    pub fn send(self, msg: Builder) -> Result<(), SendError> {
        let name = ManuallyDrop::new(self);
        send_impl(name.0, msg, MACH_MSG_TYPE_MOVE_SEND_ONCE)
    }
}

impl Drop for SendOnceRight {
    #[inline(always)]
    fn drop(&mut self) {
        self.mod_refs(-1);
    }
}

impl AsRawName for SendOnceRight {
    type Base = SendOnceRight;

    #[inline(always)]
    fn as_raw_name(&self) -> mach_port_t {
        self.0
    }
}

impl<'a> AsRawName for &'a SendOnceRight {
    type Base = SendOnceRight;

    #[inline(always)]
    fn as_raw_name(&self) -> mach_port_t {
        self.0
    }
}

impl IntoRawName for SendOnceRight {
    #[inline(always)]
    fn into_raw_name(self) -> mach_port_t {
        ManuallyDrop::new(self).0
    }
}

impl BaseRight for SendOnceRight {
    const MSG_TYPE: mach_port_right_t = MACH_MSG_TYPE_MOVE_SEND_ONCE;
}

impl BaseSendRight for SendOnceRight {}

/// A wrapper for a Mach port name that holds a receive right to a port.
#[repr(transparent)]
#[derive(Debug)]
pub struct RecvRight(mach_port_t);

impl RecvRight {
    /// Allocates a new port and returns a receive right to the newly allocated port.
    ///
    /// # Panics
    /// This function will panic in case `mach_port_allocate` returns an error. This may only happen
    /// either if the IPC space of the current task is exhausted or in case of a kernel resource
    /// shortage.
    pub fn alloc() -> Self {
        let mut raw_name = MACH_PORT_NULL;
        let result = unsafe {
            mach_port::mach_port_allocate(
                traps::mach_task_self(),
                MACH_PORT_RIGHT_RECEIVE,
                &mut raw_name,
            )
        };

        assert_eq!(result, KERN_SUCCESS);
        assert_ne!(raw_name, MACH_PORT_NULL);

        RecvRight::from_raw_name(raw_name)
    }

    /// Creates a `RecvRight` wrapper from a raw `mach_port_t`.
    #[inline(always)]
    pub fn from_raw_name(name: mach_port_t) -> Self {
        RecvRight(name)
    }

    /// Inserts a send right for the receive right into the current task and wraps the name into a
    /// [`SendRight`].
    ///
    /// # Panics
    /// This function will panic in case `mach_port_insert_right` returns an error. This should only
    /// be possible on a user reference count overflow or a kernel resource shortage.
    pub fn make_send(&self) -> SendRight {
        let raw_name = self.0;
        let result = unsafe {
            mach_port::mach_port_insert_right(
                traps::mach_task_self(),
                raw_name,
                raw_name,
                MACH_MSG_TYPE_MAKE_SEND,
            )
        };

        assert_eq!(result, KERN_SUCCESS);

        SendRight::from_raw_name(raw_name)
    }

    /// Receives a Mach message into the specified buffer.
    pub fn recv<'buffer>(
        &self,
        buffer: &'buffer mut Buffer,
    ) -> Result<MsgParser<'buffer>, RecvError> {
        let data = buffer.as_slice();
        let result = unsafe {
            mach_msg(
                data.as_ptr() as *mut mach_msg_header_t,
                MACH_RCV_MSG,
                0,
                4096,
                self.0,
                0,
                MACH_PORT_NULL,
            )
        };

        if result == KERN_SUCCESS {
            Ok(MsgParser::new(buffer))
        } else {
            Err(RecvError::from_bits(result))
        }
    }

    #[inline(always)]
    fn mod_refs(&self, delta: mach_port_delta_t) -> kern_return_t {
        mod_refs_wrapper(self.0, MACH_PORT_RIGHT_RECEIVE, delta)
    }
}

impl Drop for RecvRight {
    #[inline(always)]
    fn drop(&mut self) {
        self.mod_refs(-1);
    }
}

impl AsRawName for RecvRight {
    type Base = RecvRight;

    #[inline(always)]
    fn as_raw_name(&self) -> mach_port_t {
        self.0
    }
}

impl<'a> AsRawName for &'a RecvRight {
    type Base = RecvRight;

    #[inline(always)]
    fn as_raw_name(&self) -> mach_port_t {
        self.0
    }
}

impl IntoRawName for RecvRight {
    #[inline(always)]
    fn into_raw_name(self) -> mach_port_t {
        ManuallyDrop::new(self).0
    }
}

impl BaseRight for RecvRight {
    const MSG_TYPE: mach_port_right_t = MACH_MSG_TYPE_MOVE_RECEIVE;
}

/// An enum for all available send rights.
#[derive(Debug)]
pub enum AnySendRight {
    /// A send right.
    Send(SendRight),
    /// A send once right.
    SendOnce(SendOnceRight),
}

impl From<SendRight> for AnySendRight {
    #[inline]
    fn from(right: SendRight) -> Self {
        AnySendRight::Send(right)
    }
}

impl From<SendOnceRight> for AnySendRight {
    #[inline]
    fn from(right: SendOnceRight) -> Self {
        AnySendRight::SendOnce(right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drop() {
        // the right order
        let recv_right = RecvRight::alloc();
        let send_right = recv_right.make_send();
        drop(send_right);
        drop(recv_right);

        // the reverse order, will cause KERN_INVALID_RIGHT to be returned during the drop
        let recv_right = RecvRight::alloc();
        let send_right = recv_right.make_send();
        drop(recv_right);
        drop(send_right);
    }
}
