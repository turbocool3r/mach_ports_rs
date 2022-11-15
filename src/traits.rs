//! Contains traits for Mach port name wrappers.

use mach2::port::{mach_port_right_t, mach_port_t};

/// A trait to get a raw Mach port name (`mach_port_t`) from an object.
pub trait AsRawName {
    /// Specifies the disposition value to be set in a port descriptor when the represented right
    /// reference has to be moved to the receiver's IPC space.
    ///
    /// This should be one of the `MACH_MSG_TYPE_MOVE_*` constants.
    const MSG_TYPE: mach_port_right_t;

    /// Extracts the raw Mach port name.
    ///
    /// This function is intended to **borrow** a Mach port right reference. That is, the ownership
    /// of the reference isn't passed to the caller, and the name is only guaranteed to represent
    /// the corresponding port right only during the lifetime of the original object.
    ///
    /// This function should not alter reference counts of the port right represented by the name.
    fn as_raw_name(&self) -> mach_port_t;
}

/// A trait to convert an object into a raw Mach port name (`mach_port_t`) while taking ownership
/// of the port right reference represented by the name.
pub trait IntoRawName: AsRawName {
    /// Converts an object into a raw Mach port name.
    ///
    /// This function is intended to **pass** a Mach port right reference from a destructed object
    /// to the caller. That means the caller will be responsible for properly dropping the reference
    /// when it is no longer needed.
    ///
    /// This function should not alter reference counts of the port right represented by the name.
    fn into_raw_name(self) -> mach_port_t;
}
