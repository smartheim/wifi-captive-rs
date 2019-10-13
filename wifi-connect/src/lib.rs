#![cfg_attr(feature = "external_doc", feature(external_doc))]
#![cfg_attr(feature = "external_doc", doc(include = "../readme.md"))]
#![feature(drain_filter)]

// The library version of this crate only exposes a subset of the features for the examples.

#[macro_use]
extern crate log;

mod errors;
pub mod nm;
pub mod utils;

pub use errors::*;