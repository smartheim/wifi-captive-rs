# Wifi-Captive

> WiFi service for Linux devices that opens an access point with a captive portal for easy network configuration from your mobile phone or laptop

WiFi Connect is a utility for dynamically setting the WiFi configuration on a Linux device via a captive portal.
WiFi credentials are specified by connecting with a mobile phone or laptop to the access point that WiFi Connect creates.
The access point includes a simple DHCP server for assigning clients IP addresses
and a DNS server for routing all pages to the captive portal. 

![How it works](./docs/images/how-it-works.png?raw=true)

## Requirements and command line arguments

You need to either run the application as root or set the NET_BINDSERVICE capability like so:
`sudo setcap CAP_NET_BIND_SERVICE=+eip /path/to/binary`.
This is necessary to bind to a few "system" ports:
* port 80 for the webserver,
* port 67/68 for DHCP and
* port 53 for the dns server.

Print available command line parameters with `./wifi-captive --help`.
Command line options have environment variable counterpart.
If both a command line option and its environment variable counterpart are defined, the command line option will take higher precedence.

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

## How it works

WiFi Connect interacts via DBUS with network_manager, which must be the active network manager on the device's host OS.

### 1. Advertise: Device Creates Access Point

**Only if** no ethernet connection can be found **and** no wifi connection is configured so far:

WiFi Connect detects available WiFi networks and opens an access point with a captive portal.
Connecting to this access point with a mobile phone or laptop allows new WiFi credentials to be configured.

### 2. Connect: User Connects Phone to Device Access Point

Connect to the opened access point on the device from your mobile phone or laptop.
The access point ssid is, by default, `WiFi Connect` with no password.

### 3. Portal: Phone Shows Captive Portal to User

After connecting to the access point from a mobile phone, it will detect the captive portal and open its web page.
Opening any web page will redirect to the captive portal as well.

### 4. Credentials: User Enters Local WiFi Network Credentials on Phone

The captive portal provides the option to select a WiFi ssid from a list with detected WiFi networks or to enter
a ssid. If necessary a passphrase must be entered for the desired network.

### 5. Connected!: Device Connects to Local WiFi Network

When the network credentials have been entered, WiFi Connect will disable the access point and try to connect to the network.
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

## Similar projects

There is also Wifi-Connect from <a href="https://balena.io">balena.io</a>.
It is based on the old futures 0.1 dependency and uses Iron as http server framework
and is not designed as long running background service, but quits after a connection
is established.
It forces the user to be root (UID=0) and uses `dnsmasq` for dns and dhcp-ip provisioning. 

## Acknowledgements

* DHCP: Inspired by https://github.com/krolaw/dhcp4r (Richard Warburton).
* DNS: Inspired by https://github.com/EmilHernvall/dnsguide/blob/master/samples/sample4.rs (Emil Hernvall). 
