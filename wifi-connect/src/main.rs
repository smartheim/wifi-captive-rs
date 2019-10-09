#![feature(drain_filter)]

#[macro_use]
extern crate log;

mod config;
mod errors;
mod nm;
mod state_machine;
mod state_machine_portal_helper;
mod utils;

mod dhcp_server;
mod dns_server;
mod http_server;

pub use errors::*;
use structopt::StructOpt;
pub use utils::receive_or_exit;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let config: config::Config = config::Config::from_args();

    let mut sm = state_machine::StateMachine::StartUp(config.clone());

    loop {
        sm = if let Some(sm) = sm.progress().await? {
            sm
        } else {
            break;
        }
    }

    Ok(())
}
