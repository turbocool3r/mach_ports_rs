//! Contains traits for Mach port name wrappers.

use mach2::port::{mach_port_right_t, mach_port_t};

/// A trait to get a raw Mach port name (`mach_port_t`) from an object.
pub trait AsRawName {
    /// Specifies the right reference type of the extracted name.
    ///
    /// This may only be one of the base wrapper types:
    /// [`SendRight`](../rights/struct.SendRight.html),
    /// [`SendOnceRight`](../rights/struct.SendOnceRight.html) or
    /// [`RecvRight`](../rights/struct.RecvRight.html).
    type Base: BaseRight;

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

/// A trait only implemented by the base wrappers: [`SendRight`](../rights/struct.SendRight.html),
/// [`SendOnceRight`](../rights/struct.SendOnceRight.html) and
/// [`RecvRight`](../rights/struct.RecvRight.html).
pub trait BaseRight: IntoRawName + sealed::Sealed {
    #[doc(hidden)]
    const MSG_TYPE: mach_port_right_t;
}

mod sealed {
    use crate::rights::{RecvRight, SendOnceRight, SendRight};

    pub trait Sealed {}

    impl Sealed for RecvRight {}
    impl Sealed for SendRight {}
    impl Sealed for SendOnceRight {}
}
