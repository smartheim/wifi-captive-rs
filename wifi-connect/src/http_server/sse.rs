//! Server-Sent Events (SSE).
//!
//! SSE allow pushing events to browsers over HTTP without polling.
//! This library uses async hyper to support many concurrent push
//! connections. It supports multiple parallel channels.

use hyper::{Body, Chunk, Response};
use std::net::IpAddr;

use crate::nm::WifiConnectionEvent;
use std::collections::LinkedList;

pub type Clients = LinkedList<Client>;

#[derive(Debug)]
pub struct Client {
    tx: hyper::body::Sender,
    dest: IpAddr,
}

pub fn new() -> Clients {
    LinkedList::new()
}

pub fn ping(clients: &mut Clients) {
    push_to_all_clients(clients, "retry: 3000\nevent: ping\ndata: {}\n\n".to_owned());
}

pub fn close_all(clients: &mut Clients) {
    for client in clients.drain_filter(|_| true) {
        client.tx.abort();
    }
}

pub fn send_wifi_connection(
    clients: &mut Clients,
    message: &WifiConnectionEvent,
) -> Result<(), serde_json::error::Error> {
    let message = format!(
        "retry: 3000\nevent: {}\ndata: {}\n\n",
        message.event.to_string(),
        serde_json::to_string(&message.connection)?
    );
    push_to_all_clients(clients, message);
    Ok(())
}

/// Push a message for the event to all clients registered on the channel.
///
/// The message is first serialized and then send to all registered
/// clients on the given channel, if any.
///
/// Returns an error if the serialization fails.
fn push_to_all_clients(
    clients: &mut Clients,
    chunk: String,
) {

    // Clean up non reachable clients
    let drained = clients.drain_filter(|client| {
        let result = client.tx.try_send_data(Chunk::from(chunk.clone()));
        match result {
            Err(_) => true,
            _ => false
        }
    });
    for client in drained {
        info!("SSE Client drop: {:?}", &client.dest);
        client.tx.abort();
    }
}

/// Initiate a new SSE stream for the given request and request IP.
/// Each IP can only have one stream. If there is already an existing one,
/// the old one will be closed and overwritten.
pub fn create_stream(
    clients: &mut Clients,
    src: IpAddr,
) -> Response<Body> {
    let (sender, body) = Body::channel();

    let drained = clients.drain_filter(|client| client.dest == src);
    for client in drained {
        client.tx.abort();
    }
    clients.push_back(Client {
        tx: sender,
        dest: src,
    });

    info!("SSE Client added: {:?}. Clients: {}", src, clients.len());

    Response::builder()
        .header("connection", "keep-alive")
        .header("cache-control", "no-cache")
        .header("x-accel-buffering", "no")
        .header("content-type", "text/event-stream")
        .header("access-control-allow-origin", "*")
        .body(body)
        .expect("Could not create response")
}
