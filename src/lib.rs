#![doc = include_str!("../README.md")]
#![feature(pointer_is_aligned)]
#![feature(const_option)]
#![feature(strict_provenance)]
#![warn(missing_docs)]
#![warn(missing_debug_implementations)]
#![warn(missing_copy_implementations)]

extern crate core;

pub mod msg;
pub mod rights;
pub mod traits;
