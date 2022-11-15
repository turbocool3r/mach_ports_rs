//! Contains the implementation of the `MsgBuilder` structure used to build Mach messages.

use crate::{
    msg::{
        buffer::MsgBuffer,
        parser::{self, TransmutedMsgDesc},
        MachMsgBits, MsgId,
    },
    rights::*,
    traits::{AsRawName, IntoRawName},
};
use mach2::{
    message::*,
    port::{mach_port_t, MACH_PORT_NULL},
};
use std::{marker::PhantomData, mem, slice};

/// Converts any sized type into a byte slice.
///
/// # SAFETY
/// This function may cause UB in case the type contains any padding.
#[inline(always)]
unsafe fn anything_as_bytes<T: Sized>(anything: &T) -> &[u8] {
    let data = anything as *const T as *const u8;
    let len = mem::size_of::<T>();
    slice::from_raw_parts(data, len)
}

fn drop_header(header: &mut mach_msg_header_t) {
    let bits = MachMsgBits::from_bits(header.msgh_bits);

    if header.msgh_local_port != MACH_PORT_NULL {
        let raw_name = mem::replace(&mut header.msgh_local_port, MACH_PORT_NULL);

        match bits.local() {
            MACH_MSG_TYPE_MOVE_SEND => drop(SendRight::from_raw_name(raw_name)),
            MACH_MSG_TYPE_MOVE_SEND_ONCE => drop(SendOnceRight::from_raw_name(raw_name)),
            MACH_MSG_TYPE_COPY_SEND | MACH_MSG_TYPE_MAKE_SEND | MACH_MSG_TYPE_MAKE_SEND_ONCE => (),
            _ => unreachable!("unexpected local port bits"),
        }
    }

    if header.msgh_voucher_port != MACH_PORT_NULL {
        let raw_name = mem::replace(&mut header.msgh_voucher_port, MACH_PORT_NULL);

        match bits.local() {
            MACH_MSG_TYPE_MOVE_SEND => drop(SendRight::from_raw_name(raw_name)),
            MACH_MSG_TYPE_COPY_SEND => (),
            _ => unreachable!("unexpected voucher port bits"),
        }
    }

    assert_eq!(header.msgh_remote_port, MACH_PORT_NULL);
    assert_eq!(bits.remote(), 0);

    header.msgh_bits = MachMsgBits::new(bits.complex(), 0, 0, 0).0;
}

/// A Mach message builder.
///
/// The builder is append/insert-only so removing data from the message isn't possible since it
/// would require additional parsing.
///
/// # Right references
/// This structure accepts Mach port names in three different ways:
/// * `(append|set)_made_*` functions accept a reference to a `RecvRight` without altering its
/// receive right reference count. When a message is sent, a send or a send once right name is
/// created in the receiver's IPC space using the receive right supplied by the sender.
/// * `(append|set)_copied_*` functions accept a reference to a `SendRight` without altering its
/// send right reference count. When a message is sent, the receiver gets a reference on the send
/// right and a name is allocated for the port in its IPC space if there wasn't one before.
/// * `(append|set)_moved_*` functions consume any of the Mach port name wrappers. The reference
/// count on the corresponding rights aren't changed, but dropping the `MsgBuilder` or sending the
/// message will cause the sender to pass one reference on the right to the receiver.
#[derive(Debug)]
pub struct MsgBuilder<'a, 'buffer> {
    buffer: &'buffer mut MsgBuffer,
    inline_data_off: mach_msg_size_t,
    _marker: PhantomData<&'a ()>,
}

impl<'a, 'buffer> MsgBuilder<'a, 'buffer> {
    /// Creates a new message builder.
    pub fn new(buffer: &'buffer mut MsgBuffer) -> Self {
        Self {
            buffer,
            inline_data_off: 0,
            _marker: Default::default(),
        }
    }

    /// Sets the `msgh_id` field in the message header.
    pub fn set_id(&mut self, id: MsgId) {
        self.buffer.header_mut().msgh_id = id;
    }

    fn release_reply_port(&mut self) {
        let header = self.buffer.header_mut();
        let raw_old_name = header.msgh_local_port;
        let bits = MachMsgBits::from_bits(header.msgh_bits);

        if raw_old_name != MACH_PORT_NULL {
            match bits.local() {
                MACH_MSG_TYPE_MOVE_SEND => drop(SendRight::from_raw_name(raw_old_name)),
                MACH_MSG_TYPE_MOVE_SEND_ONCE => drop(SendOnceRight::from_raw_name(raw_old_name)),
                _ => (),
            }
        }

        header.msgh_local_port = MACH_PORT_NULL;
    }

    /// Sets the reply port right to be made from a receive right when the message is sent. The
    /// receive right stays owned by the sender.
    pub fn set_made_reply_port(&mut self, recv_right: &'a RecvRight, once: bool) {
        self.release_reply_port();

        let header = self.buffer.header_mut();
        let bits = MachMsgBits::from_bits(header.msgh_bits);
        let local_bits = if once {
            MACH_MSG_TYPE_MAKE_SEND_ONCE
        } else {
            MACH_MSG_TYPE_MAKE_SEND
        };

        header.msgh_remote_port = recv_right.as_raw_name();
        header.msgh_bits = bits.set_local(local_bits).0;
    }

    /// Sets the reply port right to be copied from a send right when the message is sent. The
    /// sender's reference on the send right isn't dropped.
    pub fn set_copied_reply_port(&mut self, name: &'a SendRight) {
        self.release_reply_port();

        let header = self.buffer.header_mut();
        let bits = MachMsgBits::from_bits(header.msgh_bits);

        header.msgh_remote_port = name.as_raw_name();
        header.msgh_bits = bits.set_local(MACH_MSG_TYPE_COPY_SEND).0;
    }

    /// Consumes a send or a send once right and sets it to be transferred to the receiver as the
    /// reply port when the message is sent.
    pub fn set_moved_reply_port<T: Into<AnySendRight>>(&mut self, reply_port: T) {
        self.release_reply_port();

        let reply_port = reply_port.into();
        let header = self.buffer.header_mut();
        let bits = MachMsgBits::from_bits(header.msgh_bits);
        let local_bits = match reply_port {
            AnySendRight::Send(send) => {
                header.msgh_remote_port = send.into_raw_name();
                MACH_MSG_TYPE_MOVE_SEND
            }
            AnySendRight::SendOnce(send_once) => {
                header.msgh_remote_port = send_once.into_raw_name();
                MACH_MSG_TYPE_MOVE_SEND_ONCE
            }
        };

        let new_bits = MachMsgBits::new(bits.complex(), 0, local_bits, bits.voucher());
        header.msgh_bits = new_bits.0;
    }

    /// Appends contents of a descriptor to the message.
    fn append_descriptor(&mut self, bytes: &[u8]) {
        debug_assert!(bytes.len() >= mem::size_of::<mach_msg_port_descriptor_t>());

        self.inc_desc_count(bytes.len());

        let appended_len: mach_msg_size_t = bytes.len().try_into().unwrap();
        self.buffer.insert(self.inline_data_off, bytes);
        self.inline_data_off += appended_len;
    }

    /// Increments the descriptor count in the message and reserves the specified amount of bytes
    /// for a descriptor. In case there were no descriptors in the message, the count is inserted
    /// after the header and the complex bit is set.
    fn inc_desc_count(&mut self, reserve_size: usize) {
        const SIZE_SIZE: usize = mem::size_of::<mach_msg_size_t>();
        let header = self.buffer.header_mut();
        let bits = MachMsgBits::from_bits(header.msgh_bits);

        if bits.complex() {
            let bytes: &mut [u8; SIZE_SIZE] = (&mut self.buffer.body_mut()[..SIZE_SIZE])
                .try_into()
                .unwrap();
            let count = mach_msg_size_t::from_ne_bytes(*bytes) + 1;
            *bytes = count.to_ne_bytes();

            self.buffer.reserve(reserve_size.try_into().unwrap());
        } else {
            // set the complex bit in the header
            header.msgh_bits = bits.into_complex().0;

            // insert a descriptor count after the header
            let count: mach_msg_size_t = 1;
            self.buffer
                .reserve((reserve_size + SIZE_SIZE).try_into().unwrap());
            self.buffer.insert(0, &count.to_ne_bytes());

            // update the inline data offset
            debug_assert_eq!(self.inline_data_off, 0);
            self.inline_data_off = SIZE_SIZE.try_into().unwrap();
        }
    }

    /// Appends a port descriptor to the message that will contain a send or a send once right to
    /// the port represented by a receive right.
    pub fn append_made_send_right(&mut self, right: &'a RecvRight, once: bool) {
        let disposition = if once {
            MACH_MSG_TYPE_MAKE_SEND_ONCE
        } else {
            MACH_MSG_TYPE_MAKE_SEND
        };
        let desc = mach_msg_port_descriptor_t::new(right.as_raw_name(), disposition);

        self.append_descriptor(unsafe { anything_as_bytes(&desc) });
    }

    /// Appends a port descriptor to the message that will contain a send right to the port
    /// represented by a send right. The provided send right's reference is not consumed.
    pub fn append_copied_send_right(&mut self, right: &'a SendRight) {
        let desc = mach_msg_port_descriptor_t::new(right.as_raw_name(), MACH_MSG_TYPE_COPY_SEND);

        self.append_descriptor(unsafe { anything_as_bytes(&desc) });
    }

    /// Appends a port descriptor to the message that will contain a receive, a send or a send once
    /// right. One sender's reference for the right is consumed when the message is sent.
    pub fn append_moved_right<T: IntoRawName>(&mut self, right: T) {
        let desc = mach_msg_port_descriptor_t::new(right.into_raw_name(), T::MSG_TYPE);

        self.append_descriptor(unsafe { anything_as_bytes(&desc) });
    }

    /// Returns a slice with the message contents.
    pub fn as_slice(&self) -> &[u8] {
        self.buffer.as_slice()
    }

    /// Appends inline data to the end of the message.
    pub fn append_inline_data(&mut self, data: &[u8]) {
        self.buffer.append(data);
    }

    /// Inserts data at an offset from the start of the inline data.
    pub fn insert_inline_data(&mut self, at: usize, data: &[u8]) {
        let at: mach_msg_size_t = at.try_into().unwrap();
        self.buffer.insert(self.inline_data_off + at, data);
    }

    pub(crate) fn set_raw_remote_port(&mut self, name: mach_port_t, bits: mach_msg_bits_t) {
        let header = self.buffer.header_mut();
        header.msgh_remote_port = name;
        header.msgh_bits = MachMsgBits::from_bits(header.msgh_bits).set_remote(bits).0
    }
}

impl Drop for MsgBuilder<'_, '_> {
    fn drop(&mut self) {
        drop_header(self.buffer.header_mut());

        let mut count = self.buffer.descriptors_count();
        let mut offset = mem::size_of::<mach_msg_size_t>() as mach_msg_size_t;
        while count > 0 {
            use TransmutedMsgDesc::*;

            match parser::next_desc_impl(self.buffer, &mut offset, false) {
                Port(desc) => {
                    let raw_name = desc.name;
                    match desc.disposition as mach_msg_type_name_t {
                        MACH_MSG_TYPE_MOVE_SEND => drop(SendRight::from_raw_name(raw_name)),
                        MACH_MSG_TYPE_MOVE_SEND_ONCE => {
                            drop(SendOnceRight::from_raw_name(raw_name))
                        }
                        MACH_MSG_TYPE_MOVE_RECEIVE => drop(RecvRight::from_raw_name(raw_name)),
                        MACH_MSG_TYPE_COPY_SEND
                        | MACH_MSG_TYPE_COPY_RECEIVE
                        | MACH_MSG_TYPE_MAKE_SEND
                        | MACH_MSG_TYPE_MAKE_SEND_ONCE => (),
                        _ => unreachable!("invalid disposition value in a port descriptor"),
                    }
                }
                Ool(_) | OolVolatile(_) => {
                    unimplemented!("OOL descriptors are not yet implemented")
                }
                OolPorts(_) => unimplemented!("OOL ports descriptors are not yet implemented"),
            }

            count -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drop() {
        let mut buffer = MsgBuffer::with_capacity(1024);
        let right = RecvRight::alloc();
        let mut builder = MsgBuilder::new(&mut buffer);
        builder.append_made_send_right(&right, true);
        builder.append_moved_right(RecvRight::alloc());
        builder.append_inline_data(b"0123456");
        builder.insert_inline_data(4, b"1337");
    }
}
