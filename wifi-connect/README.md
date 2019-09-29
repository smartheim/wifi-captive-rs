# Wifi-Captive

> WiFi service for Linux devices that opens an access point with captive portal for easy network configuration from your mobile phone or laptop

WiFi Connect is a utility for dynamically setting the WiFi configuration on a Linux device via a captive portal. WiFi credentials are specified by connecting with a mobile phone or laptop to the access point that WiFi Connect creates.


![How it works](./docs/images/how-it-works.png?raw=true)

## Requirements and command line arguments

You need `dnsmasq` in PATH and either run the application as root or set the NET_BINDSERVICE capability like so:
`sudo setcap CAP_NET_BIND_SERVICE=+eip /path/to/binary`.

Print available command line parameters with `./wifi-captive --help`.

* `--portal-ssid SSID` (or `PORTAL_SSID` environment variable): The portals WiFi SSID. Default: `WiFi Connect`
* `--portal-passphrase PASSPHRASE` (or `PORTAL_PASSPHRASE` environment variable): A WPA2 passphrase. Must be min 8 characters. Default is: Not set.
* `--quit-after-connected`: Shuts the service down after a connection has been established (wired or wireless).
* `--wait-before-reconfigure`: Time in seconds before a connection attempt is deemed unsuccessful and the access point with captive portal is opened again. Default is 20 seconds.
* `--retry-in`: Retries to connect to a configured WiFi connection after this time in seconds. Default is 5 minutes (360 seconds). The timer is reset whenever a client connects to the captive portal.

## How it works

WiFi Connect interacts with NetworkManager, which must be the active network manager on the device's host OS.

### 1. Advertise: Device Creates Access Point

**Only if** no ethernet connection can be found **and** no wifi connection is configured so far:

WiFi Connect detects available WiFi networks and opens an access point with a captive portal.
Connecting to this access point with a mobile phone or laptop allows new WiFi credentials to be configured.

### 2. Connect: User Connects Phone to Device Access Point

Connect to the opened access point on the device from your mobile phone or laptop.
The access point SSID is, by default, `WiFi Connect` with no password.

### 3. Portal: Phone Shows Captive Portal to User

After connecting to the access point from a mobile phone, it will detect the captive portal and open its web page.
Opening any web page will redirect to the captive portal as well.

### 4. Credentials: User Enters Local WiFi Network Credentials on Phone

The captive portal provides the option to select a WiFi SSID from a list with detected WiFi networks or to enter
a SSID. If necessary a passphrase must be entered for the desired network.

### 5. Connected!: Device Connects to Local WiFi Network

When the network credentials have been entered, WiFi Connect will disable the access point and try to connect to the network.
If the connection fails, it will enable the access point for another attempt.
If it succeeds, the configuration will be saved by NetworkManager.

### 6. Connection Lost

**Only if** no ethernet connection can be found **and** no WiFi connection can be established for more than 20 seconds although one is configured:

The access point is opened again for reconfiguration, as described in *1. Advertise*.

## Not supported boards / dongles

The following dongles are known **not** to work with NetworkManager:

* Official Raspberry Pi dongle (BCM43143 chip)
* Addon NWU276 (Mediatek MT7601 chip)
* Edimax (Realtek RTL8188CUS chip)

Dongles with similar chipsets will probably not work.

## Acknowlegments

This software is based on the awesome work of <a href="https://balena.io">balena.io</a> and forked off from Wifi-Connect and network-manager-rs.
It has been heavily modified.

* It no longer uses Iron as file server, but hyper directly.
* Uses Rusts new async/await (tokio 0.2+ and futures 0.3+).
* Being a non-root user is no longer a reason to forcefully quit. The NETBIND sys capability, for hosting on port 80 is all that is needed.
* The network-manager-rs crate has been embedded and extended with ethernet support, hidden-ssid support and connection-change lister support.
* The binary is now desiged as long running background service. A captive portal and access point are enabled whenever no wired connection can be found AND no wifi connection can be established for longer than 20 seconds or no wifi connection is configured.
* structopt is used instead of clap directly.
