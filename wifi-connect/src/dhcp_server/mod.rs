pub mod options;
pub mod packet;

use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};

use options::{DhcpOption, MessageType};
use packet::*;
use std::collections::HashMap;
use std::ops::Add;
use std::time::{Duration, Instant};

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

pub struct DHCPServer {
    leases: HashMap<u32, ([u8; 6], Instant)>,
    last_lease: u8,
    lease_duration: Duration,
    exit_receiver: tokio::sync::oneshot::Receiver<()>,
    server_addr: SocketAddrV4,
    server_ip_octets: [u8; 4],
    dns_ips: [u8; 8],
    pub only_once: bool,
}

struct Sender {
    out_buf: Box<[u8; 1500]>,
    server_ip: [u8; 4],
    src: SocketAddr,
}

impl DHCPServer {
    /// The default port is 67
    pub fn new(server_addr: SocketAddrV4) -> (Self, tokio::sync::oneshot::Sender<()>) {
        // Construct the dns dhcp option. Requires two dns addresses (2*IPv4 ala 4 octets).
        // We have only one dns (the router IP itself), so copying that two times is sufficient
        let mut dns_ips: [u8; 8] = [0; 8];
        let octets = &server_addr.ip().octets();
        dns_ips[0..4].copy_from_slice(octets);
        dns_ips[4..8].copy_from_slice(octets);

        let (exit_handler, exit_receiver) = tokio::sync::oneshot::channel::<()>();

        (
            DHCPServer {
                server_addr,
                server_ip_octets: server_addr.ip().octets(),
                exit_receiver,
                leases: HashMap::new(),
                last_lease: 0,
                lease_duration: Duration::new(LEASE_DURATION_SECS as u64, 0),
                dns_ips,
                only_once: false,
            },
            exit_handler,
        )
    }

    pub async fn run(&mut self) -> Result<(), super::CaptivePortalError> {
        let socket = tokio::net::UdpSocket::bind(SocketAddr::V4(self.server_addr.clone()));
        let mut socket = socket.await?;
        socket.set_broadcast(true).unwrap();

        info!("Started dhcp server on {}", &self.server_addr);

        let mut sender = Sender {
            out_buf: Box::new([0; 1500]),
            server_ip: self.server_addr.ip().octets(),
            src: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
        };

        let mut in_buf: [u8; 1500] = [0; 1500];
        loop {
            let future =
                super::receive_or_exit(&mut socket, &mut self.exit_receiver, &mut in_buf).await?;
            match future {
                // Wait for either a received packet or the exit signal
                Some((size, socket_addr)) => {
                    if let Ok(p) = decode(&in_buf[..size]) {
                        sender.src = socket_addr;
                        match p.message_type() {
                            Ok(options::MessageType::Discover) => {
                                self.handle_discover(p, &mut sender, &mut socket).await?;
                            },
                            Ok(options::MessageType::Request) => {
                                self.handle_request(p, &mut sender, &mut socket).await?;
                            },
                            Ok(options::MessageType::Release)
                            | Ok(options::MessageType::Decline) => {
                                self.handle_release(p);
                            },
                            _ => {},
                        };
                    }
                },
                // Exit signal received
                None => break,
            };
            #[cfg(tests)]
            {
                if self.only_once {
                    break;
                }
            }
        }

        info!("Stopped dhcp server on {}", &self.server_addr);
        Ok(())
    }

    /// Checks the packet see if it was intended for this DHCP server (as opposed to some other also on the network).
    #[inline]
    fn for_this_server(&self, packet: &Packet) -> bool {
        match packet.option(options::SERVER_IDENTIFIER) {
            None => false,
            Some(x) => (x == &self.server_ip_octets),
        }
    }

    // DHCP lease address range is server_ip[3]+1..255
    fn available(&self, chaddr: &[u8; 6], ip: &[u8; 4]) -> bool {
        // The last ip octet is a wrapped number 0..LEASE_NUM (we are only on subset 255.255.255.0)
        let pos = ip[3];
        let in_range = pos > self.server_ip_octets[3] && pos < 255;
        if !in_range {
            return false;
        }

        let ip_u32: u32 = bytes_u32!(ip);

        // Check if in lease table and if address has been taken by another client
        if let Some(x) = self.leases.get(&ip_u32) {
            if x.0 != *chaddr && !Instant::now().gt(&x.1) {
                return false;
            }
        }

        return true;
    }

    fn current_lease(&self, chaddr: &[u8; 6]) -> Option<u32> {
        for (i, v) in self.leases.iter() {
            if &v.0 == chaddr {
                return Some(*i);
            }
        }
        return None;
    }

    async fn handle_discover(
        &mut self,
        in_packet: packet::Packet<'_>,
        sender: &mut Sender,
        socket: &mut tokio::net::UdpSocket,
    ) -> Result<usize, std::io::Error> {
        // Prefer client's choice if available
        let ip = in_packet
            .option(options::REQUESTED_IP_ADDRESS)
            .and_then(|r| {
                if r.len() == 4 {
                    let mut client_preferred_ip: [u8; 4] = Default::default();
                    client_preferred_ip.copy_from_slice(&r[0..4]);

                    if self.available(&in_packet.chaddr, &client_preferred_ip) {
                        Some(client_preferred_ip)
                    } else {
                        None
                    }
                } else {
                    None
                }
            });

        // Otherwise prefer existing (including expired if available)
        let ip = ip.or_else(|| {
            self.current_lease(&in_packet.chaddr)
                .and_then(|ip| Some(u32_bytes!(ip)))
        });

        // Otherwise choose free ip if available
        let ip = ip.or_else(|| {
            let mut result = None;
            for _ in 0..LEASE_NUM {
                let mut ip_offer = self.server_ip_octets.clone();
                // Start with one number higher than server ip + lease offset
                self.last_lease = (self.last_lease + 1) % LEASE_NUM;
                ip_offer[3] = ip_offer[3] + self.last_lease;

                if self.available(&in_packet.chaddr, &ip_offer) {
                    result = Some(ip_offer);
                    break;
                }
            }
            result
        });

        // Return reply if ip could be found
        if let Some(ip) = ip {
            let request_options = in_packet
                .option(options::PARAMETER_REQUEST_LIST)
                .unwrap_or(&[]);
            return reply(
                options::MessageType::Offer,
                lease_options(&self.server_ip_octets, &self.dns_ips, request_options),
                in_packet,
                ip,
                sender,
                socket,
            )
            .await;
        }

        Ok(0)
    }

    async fn handle_request(
        &mut self,
        in_packet: packet::Packet<'_>,
        sender: &mut Sender,
        socket: &mut tokio::net::UdpSocket,
    ) -> Result<usize, std::io::Error> {
        // Ignore requests to alternative DHCP server
        if !self.for_this_server(&in_packet) {
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
            },
        };
        if !self.available(&in_packet.chaddr, &req_ip) {
            return reply(
                options::MessageType::Nak,
                nak_options(b"Requested IP not available"),
                in_packet,
                [0, 0, 0, 0],
                sender,
                socket,
            )
            .await;
        }
        {
            self.leases.insert(
                bytes_u32!(req_ip),
                (in_packet.chaddr, Instant::now().add(self.lease_duration)),
            );
        }
        let request_options = in_packet
            .option(options::PARAMETER_REQUEST_LIST)
            .unwrap_or(&[]);
        reply(
            options::MessageType::Ack,
            lease_options(&self.server_ip_octets, &self.dns_ips, request_options),
            in_packet,
            req_ip,
            sender,
            socket,
        )
        .await
    }

    fn handle_release(&mut self, in_packet: packet::Packet<'_>) {
        // Ignore requests to alternative DHCP server
        if !self.for_this_server(&in_packet) {
            return;
        }
        if let Some(ip) = self.current_lease(&in_packet.chaddr) {
            self.leases.remove(&ip);
        }
    }
}

fn lease_options<'a>(
    router_ip: &'a [u8; 4],
    dns_ips: &'a [u8; 8],
    options: &[u8],
) -> Vec<DhcpOption<'a>> {
    let mut vec = Vec::new();

    vec.push(options::DhcpOption {
        code: options::IP_ADDRESS_LEASE_TIME,
        data: &LEASE_DURATION_BYTES,
    });
    if options.contains(&options::SUBNET_MASK) {
        vec.push(options::DhcpOption {
            code: options::SUBNET_MASK,
            data: &SUBNET_MASK,
        });
    }
    if options.contains(&options::ROUTER) {
        vec.push(options::DhcpOption {
            code: options::ROUTER,
            data: router_ip,
        });
    }
    if options.contains(&options::DOMAIN_NAME_SERVER) {
        vec.push(options::DhcpOption {
            code: options::DOMAIN_NAME_SERVER,
            data: dns_ips,
        });
    }
    vec
}

fn nak_options(message: &[u8]) -> Vec<DhcpOption> {
    vec![options::DhcpOption {
        code: options::MESSAGE,
        data: message,
    }]
}

/// Constructs and sends reply packet back to the client.
///
/// # Arguments
///
/// additional_options should not include DHCP_MESSAGE_TYPE nor SERVER_IDENTIFIER as these
/// are added automatically.
async fn reply(
    msg_type: options::MessageType,
    additional_options: Vec<DhcpOption<'_>>,
    req_packet: packet::Packet<'_>,
    offer_ip: [u8; 4],
    sender: &mut Sender,
    socket: &mut tokio::net::UdpSocket,
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
    socket
        .send_to(p.encode(sender.out_buf.as_mut()), &addr)
        .await
}

#[cfg(test)]
mod tests {
    use super::super::CaptivePortalError;
    use super::{options::*, packet::decode, DHCPServer, DhcpOption, Packet};
    use futures_util::future::select;
    use futures_util::future::Either;
    use futures_util::try_future::try_join;
    use pin_utils::pin_mut;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::time::Duration;
    use tokio::runtime::Runtime;
    use tokio_net::udp::UdpSocket;

    pub fn new_dhcp_discover(request_ip: [u8; 4]) -> Vec<u8> {
        let mut vec = Vec::with_capacity(1000);
        vec.resize(1000, 0);
        let mut options_buf: [u8; 10] = [0; 10];
        options_buf[0] = 1; // DHCP_MESSAGE_TYPE discover
        options_buf[1..5].clone_from_slice(&request_ip); //REQUESTED_IP_ADDRESS
        options_buf[6] = SUBNET_MASK; //PARAMETER_REQUEST_LISTx4;
        options_buf[7] = ROUTER;
        options_buf[8] = DOMAIN_NAME;
        options_buf[9] = DOMAIN_NAME_SERVER;

        let p = Packet {
            reply: false,
            hops: 0,
            xid: [1, 2, 3, 4],
            secs: 0,
            broadcast: false,
            ciaddr: [0, 0, 0, 0],
            yiaddr: [0, 0, 0, 0],
            siaddr: [0, 0, 0, 0],
            giaddr: [0, 0, 0, 0],
            chaddr: [0, 0, 0, 0, 0, 0],
            options: vec![
                DhcpOption {
                    code: DHCP_MESSAGE_TYPE,
                    data: &options_buf[0..1],
                }, // 1 octet
                DhcpOption {
                    code: REQUESTED_IP_ADDRESS,
                    data: &options_buf[1..5],
                }, // 4 octets
                DhcpOption {
                    code: PARAMETER_REQUEST_LIST,
                    data: &options_buf[6..10],
                }, // 1 per option
            ],
        };
        let d = { p.encode(vec.as_mut()).len() };
        vec.truncate(d);
        vec
    }

    pub fn new_dhcp_request(request_ip: [u8; 4], server_ip: [u8; 4]) -> Vec<u8> {
        let mut vec = Vec::with_capacity(1000);
        vec.resize(1000, 0);
        let mut options_buf: [u8; 10] = [0; 10];
        options_buf[0] = 3; // DHCP_MESSAGE_TYPE request
        options_buf[1..5].clone_from_slice(&request_ip); //REQUESTED_IP_ADDRESS
        options_buf[6..10].clone_from_slice(&server_ip); //SERVER_IDENTIFIER

        let p = Packet {
            reply: false,
            hops: 0,
            xid: [1, 2, 3, 4],
            secs: 0,
            broadcast: false,
            ciaddr: [0, 0, 0, 0],
            yiaddr: [0, 0, 0, 0],
            siaddr: server_ip.clone(),
            giaddr: [0, 0, 0, 0],
            chaddr: [0, 0, 0, 0, 0, 0],
            options: vec![
                DhcpOption {
                    code: DHCP_MESSAGE_TYPE,
                    data: &options_buf[0..1],
                }, // 1 octet
                DhcpOption {
                    code: REQUESTED_IP_ADDRESS,
                    data: &options_buf[1..5],
                }, // 4 octets
                DhcpOption {
                    code: SERVER_IDENTIFIER,
                    data: &options_buf[6..10],
                }, // 4 octets
            ],
        };
        let d = { p.encode(vec.as_mut()).len() };
        vec.truncate(d);
        vec
    }

    async fn query<'a>(
        res_buffer: &'a mut [u8],
        request_ip: [u8; 4],
        server_addr: SocketAddrV4,
    ) -> Result<Packet<'a>, CaptivePortalError> {
        let mut socket = UdpSocket::bind(("0.0.0.0", 0)).await?;

        // DHCP offer
        let packet = new_dhcp_discover(request_ip);
        socket
            .send_to(&packet, SocketAddr::V4(server_addr.clone()))
            .await?;
        let (_, _) = socket.recv_from(res_buffer).await?;
        let packet = decode(res_buffer)?;
        assert_eq!(
            &[2],
            packet.option(DHCP_MESSAGE_TYPE).expect("message_type")
        );
        assert_eq!(
            &[255, 255, 255, 0],
            packet.option(SUBNET_MASK).expect("subnet_mask")
        );
        assert_eq!(
            &server_addr.ip().octets(),
            packet.option(ROUTER).expect("router")
        );
        assert_eq!(
            &server_addr.ip().octets(),
            &packet.option(DOMAIN_NAME_SERVER).expect("dns_servers")[0..4]
        );

        // DHCP request
        let packet = new_dhcp_request(request_ip, server_addr.ip().octets());
        socket
            .send_to(&packet, SocketAddr::V4(server_addr.clone()))
            .await?;
        let (_, _) = socket.recv_from(res_buffer).await?;
        let packet = decode(res_buffer)?;

        Ok(packet)
    }

    async fn test_domain_async() {
        let socket_addr = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 43210);
        let (mut dhcp_server, exit_handler) = DHCPServer::new(socket_addr);
        dhcp_server.only_once = true;

        let server = dhcp_server.run();
        let query = async move {
            let request_ip: [u8; 4] = [192, 168, 0, 10];
            let mut res_buffer: [u8; 300] = [0; 300];
            let r = query(&mut res_buffer, request_ip, socket_addr).await?;
            assert_eq!(&r.yiaddr, &request_ip);
            exit_handler.send(()).unwrap();
            Ok(())
        };

        try_join(server, query)
            .await
            .expect("Failed to execute server or lookup");
    }

    #[test]
    fn test_domain() {
        let rt = Runtime::new().unwrap();

        let timeout = tokio_timer::sleep(Duration::from_secs(2));
        pin_mut!(timeout);
        let test = test_domain_async();
        pin_mut!(test);

        let r = rt.block_on(select(timeout, test));
        match r {
            Either::Left(_) => panic!("timeout"),
            _ => {},
        };
    }
}
