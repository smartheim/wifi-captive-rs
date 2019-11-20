#![cfg_attr(feature = "external_doc", feature(external_doc))]
#![cfg_attr(feature = "external_doc", doc(include = "../readme.md"))]
#![feature(drain_filter)]

#[macro_use]
extern crate log;

mod errors;
mod network_interface;
mod utils;

pub mod config;
pub mod portal;
pub mod state_machine;

pub mod dhcp_server;
pub mod dns_server;
pub mod http_server;

pub mod network_backend;
pub use network_backend::NetworkBackend;

pub use network_interface::*;
pub use utils::*;

/// Re-export error type
pub use errors::CaptivePortalError;
