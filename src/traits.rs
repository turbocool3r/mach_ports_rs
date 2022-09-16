//! Contains traits for Mach port name wrappers.

use mach2::port::mach_port_t;

/// A trait for everything that wraps a raw Mach port name (aka `mach_port_t`) and can be converted
/// into it.
pub trait AsRawName {
    /// Converts a type into a raw Mach port name. This function should not alter reference counts
    /// of any port rights.
    fn as_raw_name(&self) -> mach_port_t;
}

/// A trait for everything that wraps a raw Mach port name (aka `mach_port_t`) and can be converted
/// into it.
pub trait IntoRawName {
    /// Converts a type into a raw Mach port name. This function should not alter reference counts
    /// of any port rights.
    fn into_raw_name(self) -> mach_port_t;
}
