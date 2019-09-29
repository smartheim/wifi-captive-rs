#![recursion_limit = "1024"]

#[macro_use]
extern crate log;

mod config;
mod dnsmasq;
pub mod exit;
mod network;
mod network_manager;
mod server;

pub use network_manager;

use std::io::Write;
use std::path;
use std::process;
use std::sync::mpsc::channel;
use std::thread;

use failure::Error;

use config::get_config;
use exit::block_exit_signals;
use network::{init_networking, process_network_commands};
use privileges::require_root;

fn main() -> Result<(), Error> {
    env_logger::init();
    block_exit_signals()?;

    let config = get_config();

    //TODO check port 80

    init_networking()?;

    let (exit_tx, exit_rx) = channel();

    thread::spawn(move || {
        process_network_commands(&config, &exit_tx);
    });

    match exit_rx.recv() {
        Ok(result) => {
            if let Err(reason) = result {
                return Err(reason);
            }
        },
        Err(e) => {
            return Err(e.into());
        },
    }

    Ok(())
}
