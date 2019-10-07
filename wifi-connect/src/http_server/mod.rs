use hyper::header::HeaderValue;
use hyper::server::conn::AddrStream;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use std::net::{SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex, MutexGuard};

use super::errors::CaptivePortalError;
use core::fmt;
use futures_util::future::Either;
use futures_util::try_future::try_select;
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::nm::{NetworkManagerEvent, WifiConnections, WifiConnectionEvent};
use std::time::Duration;

pub(crate) mod sse;

#[derive(Deserialize, Debug)]
pub struct WifiConnectionRequest {
    ssid: String,
    identity: Option<String>,
    passphrase: Option<String>,
    hw: Option<String>,
}

type HttpServerStateSync = Arc<Mutex<HttpServerState>>;

/// The state including the wifi connection list.
pub struct HttpServerState {
    /// If the user selected a connection in the UI, this sender will be called
    connection_sender: Option<tokio::sync::oneshot::Sender<Option<WifiConnectionRequest>>>,
    pub connections: WifiConnections,
    pub server_addr: SocketAddrV4,
    pub sse: sse::Clients,
    pub refresh_request: tokio::sync::mpsc::Sender<u32>,
    pub refresh_request_receiver: tokio::sync::mpsc::Receiver<u32>,
}

/// The http server.
pub struct HttpServer {
    exit_handler: tokio::sync::oneshot::Receiver<()>,
    connection_receiver: tokio::sync::oneshot::Receiver<Option<WifiConnectionRequest>>,
    /// The server state.
    pub state: HttpServerStateSync,
    pub server_addr: SocketAddrV4,
}

#[cfg(feature = "includeui")]
/// A reference to all binary embedded ui files
const PROJECT_DIR: include_dir::Dir = include_dir!("ui");

/// The file wrapper struct deals with the fact that we either read a file from the filesystem
/// or use a binary embedded variant. That means we either allocate a vector for the file content,
/// or use a pointer to the data without any allocation.
struct FileWrapper {
    path: PathBuf,
    contents: Vec<u8>,
    embedded_file: Option<include_dir::File<'static>>,
}

impl fmt::Display for NetworkManagerEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl<'a> FileWrapper {
    #[cfg(feature = "includeui")]
    pub fn from_included(file: &include_dir::File) -> FileWrapper {
        Self {
            path: PathBuf::from(file.path),
            contents: Vec::with_capacity(0),
            embedded_file: Some(file.clone()),
        }
    }
    pub fn from_filesystem(path: &str) -> Option<FileWrapper> {
        use std::fs;
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let file = Path::new(manifest_dir).join("ui").join(path);
        fs::read(&file).ok().and_then(|buf| {
            Some(FileWrapper {
                path: file,
                contents: buf,
                embedded_file: None,
            })
        })
    }

    pub fn path(&'a self) -> &'a Path {
        match self.embedded_file {
            Some(f) => f.path(),
            None => &self.path,
        }
    }

    /// The file's raw contents.
    /// This method consumes the file wrapper
    pub fn contents(self) -> Body {
        match self.embedded_file {
            Some(f) => Body::from(f.contents),
            None => Body::from(self.contents),
        }
    }
}

async fn echo(
    state: HttpServerStateSync,
    req: Request<Body>,
    src: SocketAddr,
) -> Result<Response<Body>, CaptivePortalError> {
    let mut response = Response::new(Body::empty());

    if req.method() == Method::GET {
        if req.uri().path() == "/networks" {
            let state = state.lock().unwrap();
            let data = serde_json::to_string(&state.connections)?;
            response.headers_mut().append(
                "Content-Type",
                HeaderValue::from_str("application/json").unwrap(),
            );
            *response.body_mut() = Body::from(data);
            return Ok(response);
        } else if req.uri().path() == "/events" {
            let mut state = state.lock().unwrap();
            let result = sse::create_stream(&mut state.sse, src.ip());
            info!("clients {}", state.sse.len());
            return Ok(result);
        } else if req.uri().path() == "/refresh" {
            let mut state = state.lock().unwrap();
            let _ = state.refresh_request.send(0);
            *response.status_mut() = StatusCode::OK;
            return Ok(response);
        }

        let path = &req.uri().path()[1..];

        let file = match () {
            #[cfg(not(feature = "includeui"))]
            () => FileWrapper::from_filesystem(path),
            #[cfg(feature = "includeui")]
            () => PROJECT_DIR
                .get_file(path)
                .and_then(|f| Some(FileWrapper::from_included(&f))),
        };
        // A captive portal catches all GET requests (that accept */* or text) and redirects to the main page.
        if file.is_none() {
            if let Some(v) = req.headers().get("Accept") {
                let accept = v.to_str().unwrap();
                if accept.contains("text") || accept.contains("*/*") {
                    let state = state.lock().unwrap();
                    let redirect_loc = format!(
                        "http://{}:{}/index.html",
                        state.server_addr.ip().to_string(),
                        state.server_addr.port()
                    );
                    *response.status_mut() = StatusCode::FOUND;
                    response
                        .headers_mut()
                        .append("Location", HeaderValue::from_str(&redirect_loc).unwrap());
                    return Ok(response);
                }
            }
        }

        // Serve UI
        if let Some(file) = file {
            let mime = match file.path().extension() {
                Some(ext) => match mime_guess::from_ext(ext.to_str().unwrap()).first() {
                    Some(v) => v.to_string(),
                    None => "application/octet-stream".to_owned(),
                },
                None => "application/octet-stream".to_owned(),
            };
            info!("Serve {} for {}", mime, path);
            response
                .headers_mut()
                .append("Content-Type", HeaderValue::from_str(&mime).unwrap());
            *response.body_mut() = file.contents();
            return Ok(response);
        }
    }
    if req.method() == Method::POST && req.uri().path() == "/connect" {
        // Body is a stream of chunks of bytes.
        let mut body = req.into_body();
        let mut output = Vec::new();

        while let Some(chunk) = body.next().await {
            let bytes = chunk?.into_bytes();
            output.extend(&bytes[..]);
        }
        let parsed: WifiConnectionRequest = serde_json::from_slice(&output[..])?;
        let sender = {
            // unlock mutex as soon as possible
            let mut state = state.lock().unwrap();
            state.connection_sender.take().unwrap()
        };
        sender
            .send(Some(parsed))
            .map_err(|_| CaptivePortalError::Generic("Failed to internally route data"))?;
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
    ) {
        (
            self.exit_handler,
            self.connection_receiver,
            self.state,
            self.server_addr,
        )
    }
}

impl HttpServer {
    pub fn new(server_addr: SocketAddrV4) -> (HttpServer, tokio::sync::oneshot::Sender<()>) {
        let (tx, exit_handler) = tokio::sync::oneshot::channel::<()>();
        let (connection_sender, connection_receiver) =
            tokio::sync::oneshot::channel::<Option<WifiConnectionRequest>>();

        let (refresh_request, refresh_request_receiver) =
            tokio::sync::mpsc::channel::<u32>(1);

        (
            HttpServer {
                exit_handler,
                connection_receiver,
                server_addr: server_addr.clone(),
                state: Arc::new(Mutex::new(HttpServerState {
                    connection_sender: Some(connection_sender),
                    refresh_request,
                    refresh_request_receiver,
                    connections: WifiConnections(Vec::new()),
                    server_addr,
                    sse: sse::new(),
                })),
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
        let (exit_handler, connection_receiver, state, server_addr) = self.into();

        // We need a cloned state for each future in this method
        let state_for_ping = state.clone();

        let make_service = make_service_fn(move |socket: &AddrStream| {
            let remote_addr = socket.remote_addr();
            // There is a future constructed in this future. Time to clone again.
            let state = state.clone();
            async move {
                let fun = service_fn(move |req| echo(state.clone(), req, remote_addr));
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
                let sleep = tokio_timer::sleep(Duration::from_secs(2));
                pin_mut!(sleep);
                // If the exit handler is called however, quit the loop
                let r = futures_util::future::select(sleep, &mut keep_alive_exit_handler).await;
                if let Either::Right(_) = r {
                    // Exit handler called
                    break;
                }
                let mut state = state_for_ping.lock().unwrap();
                sse::ping(&mut state.sse);
            }
            // After the not-so-endless loop finished: Close all server-send-event connections.
            // Without closing them, the graceful shutdown future would never resolve.
            let mut state = state_for_ping.lock().unwrap();
            sse::close_all(&mut state.sse);
        });

        let graceful = server.with_graceful_shutdown(async move {
            // We either shutdown when the exit_handler got called OR when we received a connection
            // request by the user.
            let r = try_select(exit_handler, connection_receiver)
                .await
                .ok()
                .unwrap();
            // select/try_select return an Either. If it's the right side of the Either (received connection),
            // we extract that connection and assign it to the GracefulShutdownState.
            // That object is a thread safe requested-connection wrapper and our way of communicating
            // a state out of this future.
            match r {
                Either::Right(f) => {
                    let mut shutdown_state = graceful_shutdown_state_clone.lock().unwrap();
                    *shutdown_state = f.0;
                    info!("Received connect state {:?}", *shutdown_state);
                    let _ = keep_alive_exit.send(());
                }
                _ => (),
            };
            ()
        });

        info!("Started http server on {}", &server_addr);
        graceful.await?;
        info!("Stopped http server on {}", &server_addr);

        // Extract the graceful shutdown state
        let mut state: MutexGuard<GracefulShutdownRequestState> =
            graceful_shutdown_state.lock().unwrap();
        Ok(state.take())
    }
}

/// Call this method to update, add, remove a network
pub async fn update_network(http_state: HttpServerStateSync, event: WifiConnectionEvent) {
    let mut state = http_state.lock().unwrap();
    info!("Add network {}", &event.connection.ssid);
    let ref mut connections = state.connections.0;
    match connections.iter().position(|n| n.ssid == event.connection.ssid) {
        Some(pos) => {
            match event.event {
                NetworkManagerEvent::Added => {
                    use std::mem;
                    mem::replace(connections.get_mut(pos).unwrap(), event.connection.clone());
                }
                NetworkManagerEvent::Removed => { connections.remove(pos); }
            };
        }
        None => {
            state.connections.0.push(event.connection.clone());
        }
    };
    sse::send_wifi_connection(&mut state.sse, &event).expect("json encoding failed");
}