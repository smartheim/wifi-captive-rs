pub mod options;
pub mod packet;

use std;
use std::cell::{RefCell};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};

use options::filter_options_by_req;
use options::{DhcpOption, MessageType};
use packet::*;
use std::collections::HashMap;
use std::ops::Add;
use std::time::{Duration, Instant};
use futures::future::{AbortHandle, Abortable};

/// Converts u32 to 4 bytes (Big endian)
#[macro_export]
macro_rules! u32_bytes {
    ( $x:expr ) => {
        [
            ($x >> 24) as u8,
            ($x >> 16) as u8,
            ($x >> 8) as u8,
            $x as u8,
        ]
    };
}

/// Converts 4 bytes to u32 (Big endian)
#[macro_export]
macro_rules! bytes_u32 {
    ( $x:expr ) => {
        ($x[0] as u32) * (1 << 24)
            + ($x[1] as u32) * (1 << 16)
            + ($x[2] as u32) * (1 << 8)
            + ($x[3] as u32)
    };
}

// Server configuration constants
const SUBNET_MASK: [u8; 4] = [255, 255, 255, 0];
const LEASE_DURATION_SECS: u32 = 7200;
const LEASE_NUM: u8 = 100;
const LEASE_DURATION_BYTES: [u8; 4] = u32_bytes!(LEASE_DURATION_SECS);

pub struct Server {
    leases: RefCell<HashMap<u32, ([u8; 6], Instant)>>,
    last_lease: RefCell<u8>,
    lease_duration: Duration,
    abort_handle: AbortHandle,
    server_ip: Ipv4Addr,
    server_ip_octets: [u8; 4],
    dns_ips: [u8; 8],
}

struct Sender<'a> {
    out_buf: &'a mut Box<[u8; 1500]>,
    server_ip: [u8; 4],
    src: SocketAddr,
}

impl Server {
    pub fn new(server_ip: Ipv4Addr) -> Result<Self, std::io::Error> {
        // Construct the dns dhcp option. Requires two dns addresses (2*IPv4 ala 4 octets).
        // We have only one dns (the router IP itself), so copying that two times is sufficient
        let mut dns_ips: [u8; 8] = [0; 8];
        let octets = &server_ip.octets();
        dns_ips[0..3].copy_from_slice(octets);
        dns_ips[4..7].copy_from_slice(octets);

        let (abort_handle, _) = AbortHandle::new_pair();

        Ok(Server {
            server_ip,
            server_ip_octets:server_ip.octets(),
            abort_handle,
            leases: RefCell::new(HashMap::new()),
            last_lease: RefCell::new(0),
            lease_duration: Duration::new(LEASE_DURATION_SECS as u64, 0),
            dns_ips,
        })
    }

    pub async fn run(&mut self) -> Result<(), std::io::Error> {
        let mut in_buf: [u8; 1500] = [0; 1500];
        let mut out_buf = Box::new([0; 1500]);

        let bind = SocketAddr::V4(SocketAddrV4::new(self.server_ip.clone(), 67));
        let socket = tokio::net::UdpSocket::bind(bind);
        let mut socket = socket.await?;
        socket.set_broadcast(true).unwrap();

        let mut sender = Sender {
            out_buf: &mut out_buf,
            server_ip: self.server_ip.octets(),
            src: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
        };

        loop {
            let (abort_handle, abort_registration) = AbortHandle::new_pair();
            let future = Abortable::new(socket.recv_from(&mut in_buf), abort_registration);
            self.abort_handle = abort_handle;
            if let Ok(fut) = future.await {
                let (size, socket_addr) = fut?;
                if let Ok(p) = decode(&in_buf[..size]) {
                    sender.src = socket_addr;
                    handle_request(&self, p, &mut sender, &mut socket).await?;
                }
            } else {
                // Future has been aborted
                return Ok(());
            }
        }
    }

    /// Checks the packet see if it was intended for this DHCP server (as opposed to some other also on the network).
    fn for_this_server(&self, packet: &Packet) -> bool {
        match packet.option(options::SERVER_IDENTIFIER) {
            None => false,
            Some(x) => (x == &self.server_ip.octets()),
        }
    }

    // DHCP lease address range is server_ip[3]+1..255
    fn available(&self, chaddr: &[u8; 6], ip: &[u8; 4]) -> bool {
        // The last ip octet is a wrapped number 0..LEASE_NUM (we are only on subset 255.255.255.0)
        let pos = ip[3];
        let in_range = pos > self.server_ip.octets()[3] && pos < 255;
        if !in_range {
            return false;
        }

        let ip_u32: u32 = bytes_u32!(ip);

        // Check if in lease table and if address has been taken by another client
        if let Some(x) = self.leases.borrow().get(&ip_u32) {
            if x.0 != *chaddr && !Instant::now().gt(&x.1) {
                return false;
            }
        }

        return true;
    }

    fn current_lease(&self, chaddr: &[u8; 6]) -> Option<u32> {
        for (i, v) in self.leases.borrow().iter() {
            if &v.0 == chaddr {
                return Some(*i);
            }
        }
        return None;
    }
}

async fn handle_request(server: &Server, in_packet: packet::Packet<'_>, sender: &mut Sender<'_>,
                        socket: &mut tokio::net::UdpSocket) -> Result<usize, std::io::Error> {
    match in_packet.message_type() {
        Ok(options::MessageType::Discover) => {
            // Prefer client's choice if available
            if let Some(r) = in_packet.option(options::REQUESTED_IP_ADDRESS) {
                if r.len() == 4 {
                    let mut client_preferred_ip: [u8; 4] = Default::default();
                    client_preferred_ip.copy_from_slice(&r[0..3]);
                    if server.available(&in_packet.chaddr, &client_preferred_ip) {
                        return reply(
                            options::MessageType::Offer,
                            lease_options(&server.server_ip_octets, &server.dns_ips),
                            in_packet,
                            [r[0], r[1], r[2], r[3]],
                            sender,
                            socket
                        ).await;
                    }
                }
            }
            // Otherwise prefer existing (including expired if available)
            if let Some(ip) = server.current_lease(&in_packet.chaddr) {
                return reply(
                    options::MessageType::Offer,
                    lease_options(&server.server_ip_octets, &server.dns_ips),
                    in_packet,
                    u32_bytes!(ip),
                    sender,
                    socket
                ).await;
            }
            // Otherwise choose nm_dbus_generated free ip if available
            for _ in 0..LEASE_NUM {
                let mut ip_offer = sender.server_ip.clone();
                // Start with one number higher than server ip + lease offset
                let last_lease = server.last_lease.replace_with(|&mut old| old + 1  % LEASE_NUM);
                ip_offer[3] = ip_offer[3] + last_lease;

                if server.available(&in_packet.chaddr, &ip_offer) {
                    reply(
                        options::MessageType::Offer,
                        lease_options(&server.server_ip_octets, &server.dns_ips),
                        in_packet,
                        ip_offer,
                        sender,
                        socket
                    ).await?;
                    break;
                }
            }
            Ok(0)
        }

        Ok(options::MessageType::Request) => {
            // Ignore requests to alternative DHCP server
            if !server.for_this_server(&in_packet) {
                return Ok(0);
            }
            let req_ip = match in_packet.option(options::REQUESTED_IP_ADDRESS) {
                None => in_packet.ciaddr,
                Some(x) => {
                    if x.len() != 4 {
                        return Ok(0);
                    } else {
                        [x[0], x[1], x[2], x[3]]
                    }
                }
            };
            if !server.available(&in_packet.chaddr, &req_ip) {
                return reply(
                    options::MessageType::Nak,
                    nak_options(b"Requested IP not available"),
                    in_packet,
                    [0, 0, 0, 0],
                    sender,
                    socket
                ).await;
            }
            server.leases.borrow_mut().insert(
                bytes_u32!(req_ip),
                (in_packet.chaddr, Instant::now().add(server.lease_duration)),
            );
            reply(
                options::MessageType::Ack,
                lease_options(&server.server_ip_octets, &server.dns_ips),
                in_packet,
                req_ip,
                sender,
                socket
            ).await
        }

        Ok(options::MessageType::Release) | Ok(options::MessageType::Decline) => {
            // Ignore requests to alternative DHCP server
            if !server.for_this_server(&in_packet) {
                return Ok(0);
            }
            if let Some(ip) = server.current_lease(&in_packet.chaddr) {
                server.leases.borrow_mut().remove(&ip);
            }
            Ok(0)
        }
        _ => Ok(0),
    }
}

fn lease_options<'a>(router_ip: &'a [u8; 4], dns_ips: &'a [u8; 8]) -> Vec<DhcpOption<'a>> {
    vec![
        options::DhcpOption {
            code: options::IP_ADDRESS_LEASE_TIME,
            data: &LEASE_DURATION_BYTES,
        },
        options::DhcpOption {
            code: options::SUBNET_MASK,
            data: &SUBNET_MASK,
        },
        options::DhcpOption {
            code: options::ROUTER,
            data: router_ip,
        },
        options::DhcpOption {
            code: options::DOMAIN_NAME_SERVER,
            data: dns_ips,
        },
    ]
}

fn nak_options(message: &[u8]) -> Vec<DhcpOption> {
    vec![options::DhcpOption {
        code: options::MESSAGE,
        data: message,
    }]
}

/// Constructs and sends reply packet back to the client.
/// additional_options should not include DHCP_MESSAGE_TYPE nor SERVER_IDENTIFIER as these
/// are added automatically.
async fn reply(
    msg_type: options::MessageType,
    additional_options: Vec<DhcpOption<'_>>,
    req_packet: packet::Packet<'_>,
    offer_ip: [u8; 4],
    sender: &mut Sender<'_>,
    socket: &mut tokio::net::UdpSocket
) -> std::io::Result<usize> {
    let ciaddr = match msg_type {
        MessageType::Nak => [0, 0, 0, 0],
        _ => req_packet.ciaddr,
    };

    let mt = &[msg_type as u8];

    let mut opts: Vec<DhcpOption> = Vec::with_capacity(additional_options.len() + 2);
    opts.push(DhcpOption {
        code: options::DHCP_MESSAGE_TYPE,
        data: mt,
    });
    opts.push(DhcpOption {
        code: options::SERVER_IDENTIFIER,
        data: &sender.server_ip,
    });
    opts.extend(additional_options);

    if let Some(prl) = req_packet.option(options::PARAMETER_REQUEST_LIST) {
        filter_options_by_req(&mut opts, &prl);
    }

    // Encodes and sends DHCP packet back to the client.
    let p = Packet {
        reply: true,
        hops: 0,
        xid: req_packet.xid,
        secs: 0,
        broadcast: req_packet.broadcast,
        ciaddr,
        yiaddr: offer_ip,
        siaddr: [0, 0, 0, 0],
        giaddr: req_packet.giaddr,
        chaddr: req_packet.chaddr,
        options: opts,
    };
    let mut addr = sender.src;
    if p.broadcast || addr.ip() == IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)) {
        addr.set_ip(IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)));
    }
    socket.send_to(p.encode(sender.out_buf.as_mut()), &addr).await
}
