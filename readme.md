# Wifi-Captive

> WiFi service for Linux devices that opens an access point with a captive portal for easy network configuration from your mobile phone or laptop

**Release note: This crate is not yet on crates.io. It relies on a few async/await modifications to
the dbus crate. As soon as those are upstream a release is made.**

WiFi Connect is a utility for dynamically setting the WiFi configuration on a Linux device via a captive portal.
WiFi credentials are specified by connecting with a mobile phone or laptop to the access point that WiFi Connect creates.
The access point includes a simple DHCP server for assigning clients IP addresses
and a DNS server for routing all pages to the captive portal. 

## Table of Contents

1. [Usage](#usage)
1. [How it works](#how-it-works)
1. [Not supported boards / dongles](#not-supported-boards-/-dongles)
1. [Development, Get Involved](#development,-get-involved)
    1. [System ports](#system-ports)
    1. [Maintenance and future development](#maintenance-and-future-development)
1. [Acknowledgements](#acknowledgements)
    1. [Similar projects](#similar-projects)
1. [FAQ](#faq)

## Usage

**Compile** with `cargo build`. You need at least rust 1.39 or rust nightly after 2019.09.

**Start** with `RUST_LOG=info cargo run -- -l 3000 -g 127.0.0.1 --dns-port 1535 --dhcp-port 6767`,
which doesn't require any permissions. The hotspot gateway is 127.0.0.1,
the http portal will be on http://127.0.0.1:3000, the dns server is on 1535,
the dhcp server on 6767. Logging is controlled by the `RUST_LOG` env variable.
It can be set to DEBUG, INFO, WARN, ERROR. Default is ERROR.

### Command line options

If both a command line option and an environment variable counterpart (identified by a leading $) is defined,
the command line option will take higher precedence.

*   **--help**

    Print available command line parameters
    
*   **-d, --portal-dhcp-range** dhcp_range, **$PORTAL_DHCP_RANGE**

    DHCP range of the captive portal WiFi network

    Default: _192.168.42.2,192.168.42.254_

*   **-g, --portal-gateway** gateway, **$PORTAL_GATEWAY**

    Gateway of the captive portal WiFi network

    Default: _192.168.42.1_

*   **-o, --portal-listening-port** listening_port, **$PORTAL_LISTENING_PORT**

    Listening port of the captive portal web server

    Default: _80_

*   **-i, --portal-interface** interface, **$PORTAL_INTERFACE**

    Wireless network interface to be used by WiFi Connect

*   **-p, --portal-passphrase** passphrase, **$PORTAL_PASSPHRASE**

    WPA2 Passphrase of the captive portal WiFi network

    Default: _no passphrase_

*   **-s, --portal-ssid** ssid, **$PORTAL_SSID**

    ssid of the captive portal WiFi network

    Default: _WiFi Connect_
    
*   **-w, --wait-before-reconfigure** sec, **$PORTAL_WAIT**

    Time in seconds before the portal is opened for re-configuration,
    if no connection can be established.

    Default: _20_

*   **-r, --retry-in** sec, **$PORTAL_RETRY_IN**

    Time in seconds before retrying to connect to a configured WiFi SSID.
    The attempt happens independently if a portal is currently open or not,
    but if a portal and access point is set up, it will be temporarily shut down
    for the connection attempt.
    The timer is reset whenever a client connects to the captive portal.

    Default: _360_

*   **-q, --quit-after-connected**

    Exit after a connection has been established. 

    Default: _false_

*   **--internet-connectivity**

    Require internet connectivity to deem a connection successful.
    Usually it is sufficient if a connection to the local network can be established.

    Default: _false_

## How it works

WiFi Connect interacts via DBUS with network_manager, which must be the active network manager on the device's host OS.

### 1. Device Creates Access Point

**Only if** no ethernet connection can be found **and** no wifi connection is configured so far:

The application detects available WiFi networks and opens an access point with a captive portal.

### 2. User Connects Phone to Device Access Point

Connect to the opened access point on the device from your mobile phone or laptop.
The access point ssid is, by default, `WiFi Connect` with no password.

### 3. Phone Shows Captive Portal to User

After connecting to the access point from a mobile phone, it will detect the captive portal and open its web page.
Opening any web page will redirect to the captive portal as well.

### 4. User Enters Local WiFi Network Credentials

The captive portal provides the option to select a WiFi ssid from a list with detected WiFi networks or to enter
a ssid. If necessary a passphrase must be entered for the desired network.

### 5. Device Connects to Local WiFi Network

When the network credentials have been entered,
the service will disable the access point and try to connect to the network.
If the connection fails, it will enable the access point for another attempt.
If it succeeds, the configuration will be saved by network_manager.

### 6. Connection Lost

**Only if** no ethernet connection can be found **and** no WiFi connection can be established for more than 20 seconds although one is configured:

The access point is opened again for reconfiguration, as described in *1. Advertise*.

## Not supported boards / dongles

The following dongles are known **not** to work with network_manager:

* Official Raspberry Pi dongle (BCM43143 chip)
* Addon NWU276 (Mediatek MT7601 chip)
* Edimax (Realtek RTL8188CUS chip)

Dongles with similar chipsets will probably not work.

## Development, Get Involved

PRs are welcome. A PR is expected to be under the same license as the crate itself.
This crate is using rusts async / await support (since Rust 1.38).
Tokio 0.2 and futures 0.3 are used. A futures 0.1 compat dependency will not
make a good PR candidate ;)

There is not yet a full integration test. IMO a good one would fake network manager
responses which requires a dbus service. The dbus crate is currently (as of Oct 2019)
restructuring how dbus services are written.  

### System ports

The default ports as mentioned above are:

* port 80 for the webserver,
* port 67/68 for DHCP and
* port 53 for the dns server.

Those ports are considered "system" ports and require elevated permissions.
You need to either run the application as root or set the NET_BINDSERVICE capability like so:
`sudo setcap CAP_NET_BIND_SERVICE=+eip /path/to/binary`.

Because this is tedious during development, you can use the helper program *set_net_cap* in `scripts`.
Use it like this: `./scripts/set_net_cap target/debug/wifi-captive`. Just add it as a last build step to your development environment.

It makes use of the fact that a setuid program doesn't require you to enter a password.
To compile the C program, change the ownership to the root user and set the setuid bit,
do this:

```shell
gcc -o scripts/set_net_cap scripts/set_net_cap.c && \
sudo chown root:root scripts/set_net_cap && \
sudo chmod +s scripts/set_net_cap
``` 

### Maintenance and future development

The application is considered almost finished. It will be adapted to newer
dependency and rust versions. 

"Almost", because one goal is, to statically compile the binary.
This is not yet possible due to the dbus crate using the libdbus C-library.

## Acknowledgements

* DHCP: Inspired by https://github.com/krolaw/dhcp4r (Richard Warburton).
  The implemented version in this crate is rewritten with only the packet struct being similar. 
* DNS: Inspired by https://github.com/EmilHernvall/dnsguide/blob/master/samples/sample4.rs (Emil Hernvall). 
  The implemented version in this crate is rewritten. The Query, Record, Header and Packet
  data structures are similar. 

### Similar projects

There is also Wifi-Connect from <a href="https://balena.io">balena.io</a>.
It is based on futures 0.1 and uses Iron as http server framework.
It is not designed as long running background service, but quits after a connection
has been established.
It forces the user to be root (UID=0) and uses `dnsmasq` for dns and dhcp-ip provisioning. 

## FAQ 

* **Can I configure multiple access points for fallback reasons?**
  Not per se. But you could configure once access point, disable that one
  (or move out of its range) and configure a second / third one.
  This service will try all known connections when in *reconnect* mode.
  
* **Are 2.4Ghz / 5 Ghz access points with the same SSID used interchangeably?**
  No. The user interface always shows the frequency of the selected access point
  and exactly that one is stored and used. 

* **Does scanning work during hotspot mode?**
  Many network chipsets do not support that. If a second / other wireless chipsets
  are installed, those will be used instead for scanning.
-----
 David Gräff, 2019