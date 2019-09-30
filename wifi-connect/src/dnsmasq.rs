use std::process::{Child, Command};

use super::network_manager::Device;

use crate::Config;
use failure::Error;

pub fn test_dnsmasq() -> bool {
    let r = Command::new("dnsmasq")
        .args(["-v"].iter())
        .output();

    if r.is_err() {
        return false;
    }

    let r = String::from_utf8(r.unwrap().stdout);
    if r.is_err() {
        return false;
    }
    let r = r.unwrap();
    r.contains("Dnsmasq version")
}

pub fn start_dnsmasq(config: &Config, device: &Device) -> Result<Child, Error> {
    let args = [
        &format!("--address=/#/{}", config.gateway),
        &format!("--dhcp-range={}", config.dhcp_range),
        &format!("--dhcp-option=option:router,{}", config.gateway),
        &format!("--interface={}", device.interface()),
        "--keep-in-foreground",
        "--bind-interfaces",
        "--except-interface=lo",
        "--conf-file",
        "--no-hosts",
    ];

    Command::new("dnsmasq")
        .args(&args)
        .spawn()
        .map_err(|_e| failure::format_err!("Dnsmasq failed"))
}
