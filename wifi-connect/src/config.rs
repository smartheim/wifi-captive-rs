use std::net::Ipv4Addr;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt, Debug)] //
pub struct Config {
    /// Wireless network interface to be used by WiFi Connect
    #[structopt(short, long = "portal-interface", env = "PORTAL_INTERFACE")]
    pub interface: Option<String>,

    /// ssid of the captive portal WiFi network
    #[structopt(
        short,
        long = "portal-ssid",
        default_value = "OHX WiFi Connect",
        env = "PORTAL_SSID"
    )]
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

    /// DHCP range of the WiFi network
    #[structopt(
        short,
        long = "portal-dhcp-range",
        default_value = "192.168.42.2,192.168.42.254",
        env = "PORTAL_DHCP_RANGE"
    )]
    pub dhcp_range: String,

    /// Listening port of the captive portal web server
    #[structopt(
        short,
        long = "portal-listening-port",
        default_value = "80",
        env = "PORTAL_LISTENING_PORT"
    )]
    pub listening_port: u16,

    /// Exit if no activity for the specified time (seconds)
    #[structopt(short, long, default_value = "0", env = "ACTIVITY_TIMEOUT")]
    pub activity_timeout: u64,

    /// A writable file where the service stores an established connection.
    /// Can be empty to not store a successfully established connection.
    /// Network manager will, if not run in stateless mode, store the last successful connection as well.
    #[structopt(
        parse(from_os_str),
        short,
        long = "connection-store",
        env = "CONNECTION_STORE"
    )]
    pub ui_directory: Option<PathBuf>,
}
