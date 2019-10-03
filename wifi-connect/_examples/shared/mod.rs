use structopt::StructOpt;

#[derive(StructOpt, Debug)]
pub struct Config {
    /// Wireless network interface to be used by WiFi Connect
    #[structopt(short, long = "interface")]
    pub interface: Option<String>,

    /// ssid of the captive portal WiFi network
    #[structopt(short, long = "ssid")]
    pub ssid: String,

    /// WPA2 Passphrase of the captive portal WiFi network
    #[structopt(short, long = "passphrase")]
    pub passphrase: Option<String>,
}
