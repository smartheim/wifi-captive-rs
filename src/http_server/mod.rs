//! A hyper based http server that serves the "ui" directory. It also provides a server-send-event
//! endpoint at /events for live updates on discovered access points.
//!
//! ## Crossmodule usage
//! This module uses the crates error type and uses the
//! *NetworkManagerEvent*, *WifiConnections* and *WifiConnectionEvent* structs
//! of the network manager module.

use hyper::header::HeaderValue;
use hyper::server::conn::AddrStream;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use std::net::{SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex, MutexGuard};

use super::errors::CaptivePortalError;
use futures_util::future::Either;
use futures_util::try_future::try_select;
use serde::Deserialize;

use super::network_backend::NetworkBackend;
use super::network_interface::{WifiConnectionEvent, WifiConnectionEventType, WifiConnections};
use std::path::PathBuf;
use std::time::Duration;

mod file_serve;
pub(crate) mod sse;

#[derive(Deserialize, Debug)]
pub struct WifiConnectionRequest {
    /// wpa, wep, open, enterprise
    pub mode: String,
    pub ssid: String,
    pub identity: Option<String>,
    pub passphrase: Option<String>,
    pub hw: Option<String>,
}

/// The http server.
pub struct HttpServer {
    exit_handler: tokio::sync::oneshot::Receiver<()>,
    connection_receiver: tokio::sync::oneshot::Receiver<Option<WifiConnectionRequest>>,
    /// The server state.
    pub state: HttpServerStateSync,
    pub server_addr: SocketAddrV4,
    pub ui_path: PathBuf,
}

/// The http server state including the wifi connection list.
pub struct HttpServerState {
    /// If the user selected a connection in the UI, this sender will be called
    connection_sender: Option<tokio::sync::oneshot::Sender<Option<WifiConnectionRequest>>>,
    pub connections: WifiConnections,
    pub server_addr: SocketAddrV4,
    pub sse: sse::Clients,
    pub network_manager: NetworkBackend,
}

/// The thread safe wrapper around the http server state.
pub type HttpServerStateSync = Arc<Mutex<HttpServerState>>;

/// Called when the user requests a wifi list refresh via /refresh.
///
/// ## Crossmodule usage
/// This method calls into the network manager
pub async fn user_requests_wifi_list_refresh(state: HttpServerStateSync) -> StatusCode {
    let nm = match state.try_lock() {
        Ok(state) => state.network_manager.clone(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
    };
    if let Ok(_) = nm.scan_networks().await {
        StatusCode::OK
    } else {
        // Some network adapters do not allow a scan while a hotspot is running
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

/// Routes to one of the dynamic routes "/networks" (list of wifi networks),
/// "/events" (server send events), "/refresh" (requests a wifi scan) and "/connect".
/// "/connect" will exit the http server and make the future of the outer state
/// machine to resolve.
async fn http_router(
    state: HttpServerStateSync,
    ui_path: PathBuf,
    req: Request<Body>,
    src: SocketAddr,
) -> Result<Response<Body>, CaptivePortalError> {
    let mut response = Response::new(Body::empty());

    if req.method() == Method::GET {
        if req.uri().path() == "/networks" {
            let state = state.lock().expect("http state mutex lock");
            let data = serde_json::to_string(&state.connections)?;
            drop(state); // release mutex
            response
                .headers_mut()
                .append("content-type", HeaderValue::from_static("application/json"));
            *response.body_mut() = Body::from(data);
            return Ok(response);
        } else if req.uri().path() == "/events" {
            let mut state = state.lock().expect("http state mutex lock");
            let result = sse::create_stream(&mut state.sse, src.ip());
            return Ok(result);
        } else if req.uri().path() == "/refresh" {
            *response.status_mut() = user_requests_wifi_list_refresh(state.clone()).await;
            return Ok(response);
        }

        return file_serve::serve_file(&ui_path, response, &req, &state);
    }
    if req.method() == Method::POST && req.uri().path() == "/connect" {
        info!("connect1");
        // Body is a stream of chunks of bytes.
        let mut body = req.into_body();
        let mut output = Vec::new();

        while let Some(chunk) = body.next().await {
            let bytes = chunk?.into_bytes();
            output.extend(&bytes[..]);
        }
        info!("connect2");
        let parsed: WifiConnectionRequest = serde_json::from_slice(&output[..])?;
        let mut state = state.lock().expect("http state mutex lock");
        let sender = state
            .connection_sender
            .take()
            .expect("http state mutex lock");
        // release mutex as soon as possible
        drop(state);
        info!("connect3");
        sender
            .send(Some(parsed))
            .map_err(|_| CaptivePortalError::Generic("Failed to internally route data"))?;
        *response.status_mut() = StatusCode::OK;
        info!("connect4");
        return Ok(response);
    }

    *response.status_mut() = StatusCode::NOT_FOUND;
    Ok(response)
}

impl HttpServer {
    pub fn into(
        self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Receiver<Option<WifiConnectionRequest>>,
        HttpServerStateSync,
        SocketAddrV4,
        PathBuf,
    ) {
        (
            self.exit_handler,
            self.connection_receiver,
            self.state,
            self.server_addr,
            self.ui_path,
        )
    }

    /// Create a new http server. The gateway address and a clone of the network manager is required.
    /// If the ui is not compiled in, a valid ui_path must be given as well.
    ///
    /// A tuple (http_server, exit handler) is returned. Call the exit handler for a graceful shutdown.
    pub fn new(
        server_addr: SocketAddrV4,
        nm: NetworkBackend,
        ui_path: PathBuf,
    ) -> (HttpServer, tokio::sync::oneshot::Sender<()>) {
        let (tx, exit_handler) = tokio::sync::oneshot::channel::<()>();
        let (connection_sender, connection_receiver) =
            tokio::sync::oneshot::channel::<Option<WifiConnectionRequest>>();

        (
            HttpServer {
                exit_handler,
                connection_receiver,
                server_addr: server_addr.clone(),
                state: Arc::new(Mutex::new(HttpServerState {
                    connection_sender: Some(connection_sender),
                    network_manager: nm,
                    connections: WifiConnections(Vec::new()),
                    server_addr,
                    sse: sse::new(),
                })),
                ui_path,
            },
            tx,
        )
    }

    /// Consumes the server object and runs it until it receives an exit signal via
    /// the [`tokio::sync::oneshot::Sender`] returned by [`new`]. Also quits the server
    /// when
    pub async fn run(
        self: HttpServer,
    ) -> Result<Option<WifiConnectionRequest>, super::CaptivePortalError> {
        // Consume the HttpServer by destructuring into its parts
        let (exit_handler, connection_receiver, state, server_addr, ui_path) = self.into();

        // We need a cloned state for each future in this method
        let state_for_ping = state.clone();

        let make_service = make_service_fn(move |socket: &AddrStream| {
            let remote_addr = socket.remote_addr();
            // There is a future constructed in this future. Time to clone again.
            let state = state.clone();
            let ui_path = ui_path.clone();
            async move {
                let fun = service_fn(move |req| {
                    http_router(state.clone(), ui_path.clone(), req, remote_addr)
                });
                Ok::<_, hyper::Error>(fun)
            }
        });

        // Construct server and bind it
        let server = Server::bind(&SocketAddr::V4(server_addr.clone())).serve(make_service);

        // A graceful shutdown state: This only contains the wifi connection request, if any.
        type GracefulShutdownRequestState = Option<WifiConnectionRequest>;
        let graceful_shutdown_state = Arc::new(Mutex::new(GracefulShutdownRequestState::None));

        // The clone will be consumed by the graceful shutdown future
        let graceful_shutdown_state_clone = graceful_shutdown_state.clone();

        // Keep alive ping for the server send events stream.
        // As usual, also establish a quit channel. Will be called by the graceful shutdown future
        let (keep_alive_exit, keep_alive_exit_handler) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            use pin_utils::pin_mut;
            let mut keep_alive_exit_handler = keep_alive_exit_handler;
            // Endless loop to send ping events ...
            loop {
                // ... every 2 seconds
                let sleep = tokio::timer::delay_for(Duration::from_secs(2));
                pin_mut!(sleep);
                // If the exit handler is called or dropped however, quit the loop
                let r = futures_util::future::select(sleep, &mut keep_alive_exit_handler).await;
                if let Either::Right(_) = r {
                    // Exit handler called
                    break;
                }
                let mut state = state_for_ping.lock().expect("http state mutex lock");
                sse::ping(&mut state.sse);
            }
            // After the not-so-endless loop finished: Close all server-send-event connections.
            // Without closing them, the graceful shutdown future would never resolve.
            let mut state = state_for_ping.lock().expect("http state mutex lock");
            sse::close_all(&mut state.sse);
        });

        let graceful = server.with_graceful_shutdown(async move {
            // We either shutdown when the exit_handler got called OR when we received a connection
            // request by the user.
            let r = try_select(exit_handler, connection_receiver).await;

            match r {
                Err(_) => {
                    // The http exit handler has been dropped. Time to leave this future.
                    return;
                },
                Ok(r) => {
                    // select/try_select return an Either. If it's the right side of the Either (received connection),
                    // we extract that connection and assign it to the GracefulShutdownState.
                    // That object is a thread safe requested-connection wrapper and our way of communicating
                    // a state out of this future.
                    match r {
                        Either::Right((f, _receiver)) => {
                            let mut shutdown_state = graceful_shutdown_state_clone
                                .lock()
                                .expect("Mutex lock for http server state on graceful shutdown");
                            *shutdown_state = f;
                            info!("Received connect state {:?}", *shutdown_state);
                        },
                        // The http exit handler has been been activated. Time to leave this future.
                        _ => (),
                    };
                },
            }

            // Stop server-send-events keep alive and refresh request future
            let _ = keep_alive_exit.send(());
            ()
        });

        info!("Started http server on {}", &server_addr);
        graceful.await?;
        info!("Stopped http server on {}", &server_addr);

        // Extract the graceful shutdown state
        let mut state: MutexGuard<GracefulShutdownRequestState> = graceful_shutdown_state
            .lock()
            .expect("http server mutex lock for return value");
        Ok(state.take())
    }
}

/// Call this method to update, add, remove a network
pub async fn update_network(http_state: HttpServerStateSync, event: WifiConnectionEvent) {
    let mut state = http_state
        .lock()
        .expect("Mutex lock for http state on update_network");
    info!("Add network {}", &event.connection.ssid);
    let ref mut connections = state.connections.0;
    match connections
        .iter()
        .position(|n| n.ssid == event.connection.ssid)
    {
        Some(pos) => {
            match event.event {
                WifiConnectionEventType::Added => {
                    use std::mem;
                    let dest = connections
                        .get_mut(pos)
                        .expect("update_network: Vector access on connections");
                    mem::replace(dest, event.connection.clone());
                },
                WifiConnectionEventType::Removed => {
                    connections.remove(pos);
                },
            };
        },
        None => {
            state.connections.0.push(event.connection.clone());
        },
    };
    sse::send_wifi_connection(&mut state.sse, &event).expect("json encoding failed");
}
