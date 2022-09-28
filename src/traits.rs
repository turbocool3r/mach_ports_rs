//! Contains traits for Mach port name wrappers.

use mach2::port::{mach_port_right_t, mach_port_t};

/// A trait for everything that wraps a raw Mach port name (aka `mach_port_t`) and can be converted
/// into it.
pub trait AsRawName {
    /// Specifies the disposition value to be set in a port descriptor when the represented right
    /// reference has to be moved to the receiver's IPC space.
    ///
    /// This should be one of the `MACH_MSG_TYPE_MOVE_*` constants.
    const MSG_TYPE: mach_port_right_t;

    /// Converts a type into a raw Mach port name. This function should not alter reference counts
    /// of any port rights.
    fn as_raw_name(&self) -> mach_port_t;
}

/// A trait for everything that wraps a raw Mach port name (aka `mach_port_t`) and can be converted
/// into it.
pub trait IntoRawName: AsRawName {
    /// Converts a type into a raw Mach port name. This function should not alter reference counts
    /// of any port rights.
    fn into_raw_name(self) -> mach_port_t;
}
