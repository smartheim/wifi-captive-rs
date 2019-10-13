//! This is a DNS server implementation that returns the given gateway address for
//! every request. This will be recognised by most mobile phones and browsers as
//! a captive portal.

mod byte_buffer;
mod dns_header;
mod dns_packet;
mod dns_query;
mod dns_record;

use byte_buffer::BytePacketBuffer;
use dns_header::ResultCode;
use dns_packet::DnsPacket;
use dns_record::DnsRecord;

use super::CaptivePortalError;

use std::clone::Clone;
use std::net::{SocketAddr, SocketAddrV4};
use tokio::net::UdpSocket;

/// A DNS server that responds with one IP for all requests
pub struct CaptiveDnsServer {
    exit_receiver: tokio::sync::oneshot::Receiver<()>,
    server_addr: SocketAddrV4,
    /// For testing: Quits the receive loop after one received packet
    #[allow(unused)]
    only_once: bool,
}

impl CaptiveDnsServer {
    // Standard port is 53
    pub fn new(server_addr: SocketAddrV4) -> (Self, tokio::sync::oneshot::Sender<()>) {
        let (exit_handler, exit_receiver) = tokio::sync::oneshot::channel::<()>();

        (
            CaptiveDnsServer {
                server_addr,
                exit_receiver,
                only_once: false,
            },
            exit_handler,
        )
    }

    pub async fn run(&mut self) -> Result<(), CaptivePortalError> {
        let mut socket = tokio::net::UdpSocket::bind(SocketAddr::V4(self.server_addr.clone())).await?;
        socket
            .set_broadcast(true)
            .expect("Set broadcast flag on udp socket");

        info!("Started dns server on {}", &self.server_addr);

        let mut req_buffer = BytePacketBuffer::new();
        loop {
            let future =
                super::utils::receive_or_exit(&mut socket, &mut self.exit_receiver, &mut req_buffer.buf)
                    .await?;
            match future {
                // Wait for either a received packet or the exit signal
                Some((size, socket_addr)) => {
                    req_buffer.set_size(size)?;
                    if let Ok(p) = DnsPacket::from_buffer(&mut req_buffer) {
                        handle_request(&self, p, socket_addr, &mut req_buffer, &mut socket).await?;
                    }
                }
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

        drop(socket);
        info!("Stopped dns server on {}", &self.server_addr);
        Ok(())
    }
}

async fn handle_request(
    server: &CaptiveDnsServer,
    request: DnsPacket,
    src: SocketAddr,
    mut res_buffer: &mut BytePacketBuffer,
    socket: &mut UdpSocket,
) -> Result<usize, CaptivePortalError> {
    res_buffer.reset_for_write();

    let mut packet = DnsPacket::new();
    packet.header.id = request.header.id;
    packet.header.recursion_desired = true;
    packet.header.recursion_available = true;
    packet.header.response = true;

    if request.questions.is_empty() {
        packet.header.rescode = ResultCode::FORMERR;
    } else {
        let question = &request.questions[0];
        info!("Received DNS query: {:?}", question);

        packet.questions.push(question.clone());
        packet.header.rescode = ResultCode::NOERROR;

        let answer = DnsRecord::A {
            domain: question.name.clone(),
            addr: server.server_addr.ip().clone(),
            ttl: 360,
        };
        packet.answers.push(answer);
    }

    packet.write(&mut res_buffer)?;

    let len = res_buffer.pos();
    let data = res_buffer.get_range(0, len)?;
    Ok(socket.send_to(data, src).await?)
}

#[cfg(test)]
mod tests {
    use super::dns_query::QueryType;
    use super::*;
    use crate::dns_server::dns_query::DnsQuery;
    use futures_util::future::select;
    use futures_util::future::Either;
    use futures_util::try_future::try_join;
    use pin_utils::pin_mut;
    use std::net::Ipv4Addr;
    use std::time::Duration;
    use tokio::runtime::Runtime;

    async fn lookup(
        qname: &str,
        qtype: QueryType,
        server: SocketAddr,
    ) -> Result<DnsPacket, super::CaptivePortalError> {
        let mut socket = UdpSocket::bind(("0.0.0.0", 0)).await?;

        let mut packet = DnsPacket::new();

        packet.header.id = 6666;
        packet.header.questions = 1;
        packet.header.recursion_desired = true;
        packet
            .questions
            .push(DnsQuery::new(qname.to_string(), qtype));

        let mut req_buffer = BytePacketBuffer::new();
        req_buffer.reset_for_write();
        packet.write(&mut req_buffer).unwrap();
        socket
            .send_to(&req_buffer.buf[0..req_buffer.pos], server)
            .await?;

        let mut res_buffer = BytePacketBuffer::new();
        let (size, _) = socket.recv_from(&mut res_buffer.buf).await?;
        res_buffer.set_size(size)?;

        Ok(DnsPacket::from_buffer(&mut res_buffer)?)
    }

    async fn test_domain_async() {
        let socket_addr = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 43210);
        let (mut dns_server, exit_handler) = CaptiveDnsServer::new(socket_addr);
        dns_server.only_once = true;

        let server = dns_server.run();
        let lookup = async move {
            let r = lookup("www.google.com", QueryType::A, SocketAddr::V4(socket_addr)).await?;
            let r = r.answers.get(0).unwrap();
            match r {
                DnsRecord::A { domain, addr, ttl } => {
                    assert_eq!(&domain as &str, "www.google.com");
                    assert_eq!(&addr, &socket_addr.ip());
                    assert_eq!(*ttl, 360);
                    exit_handler.send(()).unwrap();
                    Ok(())
                }
                _ => Err(CaptivePortalError::Generic("Unexpected response")),
            }
        };

        try_join(server, lookup)
            .await
            .expect("Failed to execute server or lookup");
    }

    #[test]
    fn test_domain() {
        let rt = Runtime::new().unwrap();

        let timeout = tokio_timer::delay_for(Duration::from_secs(2));
        pin_mut!(timeout);
        let test = test_domain_async();
        pin_mut!(test);

        let r = rt.block_on(select(timeout, test));
        match r {
            Either::Left(_) => panic!("timeout"),
            _ => {}
        };
    }
}
