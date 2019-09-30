use failure::Error;
use nix::sys::signal::{SigSet, SIGHUP, SIGINT, SIGQUIT, SIGTERM};
use log::info;
use crate::network_manager::errors::NetworkManagerError;

/// Block exit signals from the main thread with mask inherited by children
pub fn block_exit_signals() -> Result<(), Error> {
    let mask = create_exit_sigmask();
    mask.thread_block()
        .map_err(|_| failure::format_err!("BlockExitSignals"))
}

/// Trap exit signals from a signal handling thread
pub fn trap_exit_signals() -> Result<(), NetworkManagerError> {
    let mask = create_exit_sigmask();

    let sig = mask.wait().map_err(|_e|NetworkManagerError::Generic("Signal handler trap failed"))?;

    info!("\nReceived {:?}", sig);

    Ok(())
}

pub fn create_exit_sigmask() -> SigSet {
    let mut mask = SigSet::empty();

    mask.add(SIGINT);
    mask.add(SIGQUIT);
    mask.add(SIGTERM);
    mask.add(SIGHUP);

    mask
}
