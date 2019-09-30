//use futures_util::future;
//use futures_util::stream::StreamExt;

#[macro_use]
extern crate log;

pub mod dnsmasq;
pub mod exit;
mod network;
pub mod network_manager;
pub mod server;

use std::sync::mpsc::channel;
use std::thread;

use failure::Error;

use wifi_captive::Config;
use exit::block_exit_signals;
use network::{process_network_commands};

use structopt::StructOpt;
use crate::dnsmasq::test_dnsmasq;
use wifi_captive::network::{start_network_manager_service, delete_access_point_profiles};

///#[tokio::main]
/*async*/
fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init()?;

    if !test_dnsmasq() {
        warn!("dnsmasq not found!");
        return Ok(());
    }

    block_exit_signals()?;

    let config = Config::from_args();

    //TODO check port 80

    start_network_manager_service()?;
    delete_access_point_profiles()?;

    thread::spawn(move || {
        process_network_commands(config);
    });

    Ok(())
}
