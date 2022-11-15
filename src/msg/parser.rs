//! Contains the implementation of the Mach message parser.

use crate::{
    msg::{buffer::MsgBuffer, MachMsgBits, MsgId},
    rights::{AnySendRight, RecvRight, SendOnceRight, SendRight},
};
use mach2::{message::*, port::MACH_PORT_NULL};
use std::mem;

fn size_for_desc_type(type_: mach_msg_descriptor_type_t) -> usize {
    match type_ {
        MACH_MSG_PORT_DESCRIPTOR => mem::size_of::<mach_msg_port_descriptor_t>(),
        MACH_MSG_OOL_DESCRIPTOR | MACH_MSG_OOL_VOLATILE_DESCRIPTOR => {
            mem::size_of::<mach_msg_ool_descriptor_t>()
        }
        MACH_MSG_OOL_PORTS_DESCRIPTOR => mem::size_of::<mach_msg_ool_ports_descriptor_t>(),
        _ => unreachable!("unexpected Mach message descriptor type {:#x}", type_),
    }
}

/// Converts a byte slice into any structure.
///
/// # Safety
/// This function may produce invalid type invariants. Ensuring the validity of these is the
/// responsibility of the caller.
///
/// # Panics
/// This function will panic if either byte pointer isn't properly aligned for `T` or the size of
/// the slice doesn't match the size of `T`.
unsafe fn anything_from_bytes<T: Sized>(bytes: &[u8]) -> &T {
    assert!(bytes.as_ptr().is_aligned_to(mem::align_of::<T>()));
    assert_eq!(bytes.len(), mem::size_of::<T>());

    &*(bytes.as_ptr() as *const T)
}

/// Represents a parsed message header.
#[derive(Debug)]
pub struct ParsedMsgHdr {
    /// The message ID value from the message header.
    pub id: MsgId,
    /// The reply port right passed with the message if any.
    pub reply_right: Option<AnySendRight>,
    /// The voucher port right passed with the message if any.
    pub voucher: Option<SendRight>,
}

/// Represents a parsed message descriptor.
#[derive(Debug)]
pub enum ParsedMsgDesc {
    /// A receive right from a port descriptor.
    PortRecv(RecvRight),
    /// A send right from a port descriptor.
    PortSend(SendRight),
    /// A send once right from a port descriptor.
    PortSendOnce(SendOnceRight),
}

pub(crate) enum TransmutedMsgDesc<'a> {
    Port(&'a mach_msg_port_descriptor_t),
    Ool(&'a mach_msg_ool_descriptor_t),
    OolVolatile(&'a mach_msg_ool_descriptor_t),
    OolPorts(&'a mach_msg_ool_ports_descriptor_t),
}

/// Message body parser.
#[derive(Debug)]
pub struct MsgBodyParser<'buffer> {
    buffer: &'buffer mut MsgBuffer,
    offset: mach_msg_size_t,
}

impl MsgBodyParser<'_> {
    /// Returns the message body as a byte slice.
    pub fn body(&self) -> &[u8] {
        let offset = self.offset as usize;
        let size = self.buffer.header().msgh_size as usize - mem::size_of::<mach_msg_header_t>();

        debug_assert!(offset <= size);
        debug_assert!(size <= self.buffer.body().len());

        &self.buffer.body()[offset..size]
    }
}

/// Either a descriptor or a body parser.
#[derive(Debug)]
pub enum MsgDescOrBodyParser<'buffer> {
    /// A descriptor parser.
    Descriptor(MsgDescParser<'buffer>),
    /// A body parser.
    Body(MsgBodyParser<'buffer>),
}

pub(crate) fn next_desc_impl<'buffer>(
    buffer: &'buffer mut MsgBuffer,
    offset: &mut mach_msg_size_t,
    received: bool,
) -> TransmutedMsgDesc<'buffer> {
    let cur_offset = *offset as usize;
    let body_size = if received {
        buffer.header().msgh_size as usize - mem::size_of::<mach_msg_header_t>()
    } else {
        buffer.body().len()
    };

    assert!(cur_offset < body_size);
    debug_assert!(body_size <= buffer.body().len());

    // TODO: use mach_msg_type_descriptor_t when available from mach2.
    let space_left = body_size - cur_offset;
    assert!(space_left >= mem::size_of::<mach_msg_port_descriptor_t>());
    let tail = &buffer.body()[cur_offset..];

    let type_desc: &mach_msg_port_descriptor_t =
        unsafe { anything_from_bytes(&tail[..mem::size_of::<mach_msg_port_descriptor_t>()]) };
    let type_ = type_desc.type_ as mach_msg_descriptor_type_t;

    let desc_size = size_for_desc_type(type_);
    assert!(desc_size <= space_left);
    let desc_bytes = &tail[..desc_size];

    let transmuted_desc = match type_ {
        MACH_MSG_PORT_DESCRIPTOR => {
            TransmutedMsgDesc::Port(unsafe { anything_from_bytes(desc_bytes) })
        }
        MACH_MSG_OOL_DESCRIPTOR => {
            TransmutedMsgDesc::Ool(unsafe { anything_from_bytes(desc_bytes) })
        }
        MACH_MSG_OOL_VOLATILE_DESCRIPTOR => {
            TransmutedMsgDesc::OolVolatile(unsafe { anything_from_bytes(desc_bytes) })
        }
        MACH_MSG_OOL_PORTS_DESCRIPTOR => {
            TransmutedMsgDesc::OolPorts(unsafe { anything_from_bytes(desc_bytes) })
        }
        _ => unreachable!("invalid descriptor type"),
    };

    *offset = (cur_offset + desc_size).try_into().unwrap();

    transmuted_desc
}

/// A Mach message parser received after parsing the header.
#[derive(Debug)]
pub struct MsgDescParser<'buffer> {
    buffer: Option<&'buffer mut MsgBuffer>,
    count: mach_msg_size_t,
    offset: mach_msg_size_t,
}

impl<'buffer> MsgDescParser<'buffer> {
    /// Parses the next descriptor from the message.
    pub fn next(mut self) -> (ParsedMsgDesc, MsgDescOrBodyParser<'buffer>) {
        assert!(self.count > 0);

        let parsed_desc =
            match next_desc_impl(self.buffer.as_mut().unwrap(), &mut self.offset, true) {
                TransmutedMsgDesc::Port(port_desc) => {
                    match port_desc.disposition as mach_msg_copy_options_t {
                        MACH_MSG_TYPE_MOVE_SEND => {
                            ParsedMsgDesc::PortSend(SendRight::from_raw_name(port_desc.name))
                        }
                        MACH_MSG_TYPE_MOVE_SEND_ONCE => ParsedMsgDesc::PortSendOnce(
                            SendOnceRight::from_raw_name(port_desc.name),
                        ),
                        MACH_MSG_TYPE_MOVE_RECEIVE => {
                            ParsedMsgDesc::PortRecv(RecvRight::from_raw_name(port_desc.name))
                        }
                        //MACH_MSG_TYPE_COPY_SEND | MACH_MSG_TYPE_MAKE_SEND | MACH_MSG_TYPE_MAKE_SEND_ONCE =>
                        _ => unreachable!("invalid disposition value in a port descriptor"),
                    }
                }
                TransmutedMsgDesc::Ool(_) | TransmutedMsgDesc::OolVolatile(_) => {
                    unimplemented!("OOL and volatile OOL descriptors are not yet supported")
                }
                TransmutedMsgDesc::OolPorts(_) => {
                    unimplemented!("OOL ports descriptors are not supported")
                }
            };
        self.count -= 1;

        let parser = if self.count > 0 {
            MsgDescOrBodyParser::Descriptor(self)
        } else {
            MsgDescOrBodyParser::Body(MsgBodyParser {
                buffer: self.buffer.take().unwrap(),
                offset: mem::replace(&mut self.offset, 0),
            })
        };

        (parsed_desc, parser)
    }
}

impl Drop for MsgDescParser<'_> {
    fn drop(&mut self) {
        // Iterate through all remaining descriptors and free resources.
        while self.count > 0 {
            match next_desc_impl(self.buffer.as_mut().unwrap(), &mut self.offset, true) {
                TransmutedMsgDesc::Port(port_desc) => {
                    match port_desc.disposition as mach_msg_copy_options_t {
                        MACH_MSG_TYPE_MOVE_SEND => drop(SendRight::from_raw_name(port_desc.name)),
                        MACH_MSG_TYPE_MOVE_SEND_ONCE => {
                            drop(SendOnceRight::from_raw_name(port_desc.name))
                        }
                        MACH_MSG_TYPE_MOVE_RECEIVE => {
                            drop(RecvRight::from_raw_name(port_desc.name))
                        }
                        _ => unreachable!("invalid disposition value in a port descriptor"),
                    }
                }
                TransmutedMsgDesc::Ool(_) | TransmutedMsgDesc::OolVolatile(_) => {
                    unimplemented!("OOL and volatile OOL descriptors are not yet supported")
                }
                TransmutedMsgDesc::OolPorts(_) => {
                    unimplemented!("OOL ports descriptors are not supported")
                }
            }

            self.count -= 1;
        }

        // Going through trailers and body is not required as they do not contain any resources that
        // need to be freed.
    }
}

fn parse_header_impl(buffer: &mut MsgBuffer) -> (ParsedMsgHdr, MsgDescOrBodyParser) {
    let header = buffer.header_mut();
    let bits = MachMsgBits(header.msgh_bits);
    let id = header.msgh_id;

    let raw_voucher_name = header.msgh_voucher_port;
    let voucher = if raw_voucher_name != MACH_PORT_NULL {
        assert!(matches!(
            bits.voucher(),
            MACH_MSG_TYPE_COPY_SEND | MACH_MSG_TYPE_MOVE_SEND
        ));
        Some(SendRight::from_raw_name(raw_voucher_name))
    } else {
        None
    };

    let raw_remote_port_name = header.msgh_remote_port;
    let reply_right = if raw_remote_port_name != MACH_PORT_NULL {
        Some(match bits.remote() {
            MACH_MSG_TYPE_MOVE_SEND => SendRight::from_raw_name(raw_remote_port_name).into(),
            MACH_MSG_TYPE_MOVE_SEND_ONCE => {
                SendOnceRight::from_raw_name(raw_remote_port_name).into()
            }
            _ => unreachable!("unexpected reply port rights"),
        })
    } else {
        None
    };

    let count = buffer.descriptors_count();
    let desc_parser = if count > 0 {
        MsgDescOrBodyParser::Descriptor(MsgDescParser {
            buffer: Some(buffer),
            count,
            offset: mem::size_of::<mach_msg_size_t>() as mach_msg_size_t,
        })
    } else {
        MsgDescOrBodyParser::Body(MsgBodyParser { buffer, offset: 0 })
    };

    let parsed_hdr = ParsedMsgHdr {
        id,
        reply_right,
        voucher,
    };

    (parsed_hdr, desc_parser)
}

/// A Mach message parser that can parse Mach message headers and construct subsequent parsers.
#[repr(transparent)]
#[derive(Debug)]
pub struct MsgParser<'buffer>(Option<&'buffer mut MsgBuffer>);

impl<'buffer> MsgParser<'buffer> {
    #[inline(always)]
    pub(crate) fn new(buffer: &'buffer mut MsgBuffer) -> Self {
        unsafe {
            buffer.set_len(buffer.header().msgh_size);
        }

        MsgParser(Some(buffer))
    }

    /// Parses the header of the message and returns the parsed header and either a descriptor or
    /// a body parser depending on whether there are descriptors in the message.
    pub fn parse_header(mut self) -> (ParsedMsgHdr, MsgDescOrBodyParser<'buffer>) {
        let buffer = self.0.take().unwrap();
        parse_header_impl(buffer)
    }
}

impl Drop for MsgParser<'_> {
    fn drop(&mut self) {
        if let Some(buffer) = &mut self.0 {
            drop(parse_header_impl(buffer))
        }
    }
}
