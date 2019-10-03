//use futures_util::future;
//use futures_util::stream::StreamExt;

//pub mod network_manager;
//mod network_manager_helper;

#[macro_use]
extern crate log;

mod config;
mod errors;
mod nm_dbus_generated;
mod state_machine;
mod utils;

mod dhcp_server;
mod http_server;
mod dns_server;

pub use errors::*;
use structopt::StructOpt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init()?;

    let config: config::Config = config::Config::from_args();

    let mut sm = state_machine::StateMachine::StartUp(config);

    loop {
        sm = sm.progress().await?;
        match sm {
            state_machine::StateMachine::Exit => break,
            _ => {}
        };
    }

    Ok(())
}
