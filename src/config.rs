//! The command line configuration is defined in this module.

use std::net::Ipv4Addr;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt, Debug, Clone)] //
pub struct Config {
    /// Wireless network interface to be used by WiFi Connect
    #[structopt(short, long = "interface", env = "PORTAL_INTERFACE")]
    pub interface: Option<String>,

    /// ssid of the captive portal WiFi network
    #[structopt(short, long = "portal-ssid", default_value = "OHX WiFi Connect", env = "PORTAL_SSID")]
    pub ssid: String,

    /// WPA2 Passphrase of the captive portal WiFi network
    #[structopt(short, long = "portal-passphrase", env = "PORTAL_PASSPHRASE")]
    pub passphrase: Option<String>,

    /// WPA2-Enterprise Identity for the captive portal WiFi network
    #[structopt(long = "portal-identity", env = "PORTAL_IDENTITY")]
    pub identity: Option<String>,

    /// Gateway of the captive portal WiFi network
    #[structopt(
        short,
        long = "portal-gateway",
        default_value = "192.168.42.1",
        env = "PORTAL_GATEWAY"
    )]
    pub gateway: Ipv4Addr,

    /// Listening port of the captive portal web server
    #[structopt(
        short,
        long = "portal-listening-port",
        default_value = "80",
        env = "PORTAL_LISTENING_PORT"
    )]
    pub listening_port: u16,

    /// DNS server port
    #[structopt(default_value = "53", long = "dns-port")]
    pub dns_port: u16,

    /// DHCP server port
    #[structopt(default_value = "67", long = "dhcp-port")]
    pub dhcp_port: u16,

    /// Time in seconds before the portal is opened for re-configuration, if no connection can be established.
    /// During this time, the application is listening to network manager connection state changes.
    #[structopt(short, long, default_value = "10", env = "WAIT_BEFORE_RECONFIGURE")]
    pub wait_before_reconfigure: u64,

    /// Time in seconds before retrying to connect to a configured WiFi SSID.
    /// The attempt happens independently if a portal is currently open or not,
    /// but if a portal and access point is set up, it will be temporarily shut down
    /// for the connection attempt.
    /// The timer is reset whenever a client connects to the captive portal.
    #[structopt(short, long, default_value = "360", env = "RETRY_IN")]
    pub retry_in: u64,

    /// Exit after a connection has been established.
    #[structopt(short, long)]
    pub quit_after_connected: bool,

    /// Require internet connectivity to deem a connection successful. Usually it is sufficient if a connection to the local network can be established.
    #[structopt(long)]
    pub internet_connectivity: bool,

    /// The directory where the html files reside.
    #[structopt(parse(from_os_str), short, long = "connection-store", env = "CONNECTION_STORE")]
    #[cfg(all(not(feature = "includeui"), debug_assertions))]
    pub ui_directory: Option<PathBuf>,
}

impl Config {
    #[cfg(all(not(feature = "includeui"), debug_assertions))]
    pub fn get_ui_directory(&self) -> PathBuf {
        self.ui_directory.clone().unwrap_or("ui".into())
    }

    #[cfg(any(feature = "includeui", not(debug_assertions)))]
    pub fn get_ui_directory(&self) -> PathBuf {
        PathBuf::new()
    }
}
