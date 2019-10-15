#![cfg_attr(feature = "external_doc", feature(external_doc))]
#![cfg_attr(feature = "external_doc", doc(include = "../readme.md"))]
#![feature(drain_filter)]

// The library version of this crate only exposes a subset of the features for the examples.

#[macro_use]
extern crate log;

mod errors;
mod nm;
mod utils;

pub mod lib {
    pub use super::nm::*;
    pub use super::utils::*;
}
pub use errors::CaptivePortalError;
