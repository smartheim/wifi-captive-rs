use hyper::service::{make_service_fn, service_fn};
use hyper::{Response, Request, Body, Method, StatusCode, Server};
use std::error::Error;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use hyper::server::conn::AddrStream;
use hyper::header::HeaderValue;

use super::errors::CaptivePortalError;
use serde::{Serialize, Deserialize};
use futures_util::compat::Future01CompatExt;
use futures_util::try_future::try_select;
use futures_util::TryStreamExt;
use futures_util::future::Either;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Debug)]
pub struct WifiConnectionRequest {
    ssid: String,
    identity: Option<String>,
    passphrase: Option<String>,
}

#[derive(Serialize)]
pub struct WifiConnection {
    pub ssid: String,
    pub uuid: String,
    pub security: String,
    pub strength: u8,
}

#[derive(Serialize)]
pub struct WifiConnections(pub Vec<WifiConnection>);

type HttpServerStateSync = Arc<Mutex<HttpServerState>>;

/// The state including the wifi connection list.
pub struct HttpServerState {
    connection_sender: Option<tokio::sync::oneshot::Sender<Option<WifiConnectionRequest>>>,
    pub connections: WifiConnections,
    pub server_ip: Ipv4Addr,
    pub port: u16,
    pub dest: Option<WifiConnectionRequest>,
}

/// The http server.
pub struct HttpServer {
    exit_handler: tokio::sync::oneshot::Receiver<()>,
    connection_receiver: tokio::sync::oneshot::Receiver<Option<WifiConnectionRequest>>,
    /// The server state. Update `connections` periodically.
    pub state: HttpServerStateSync,
    pub server_ip: Ipv4Addr,
    pub port: u16,
}

#[cfg(feature = "includeui")]
#[macro_use]
extern crate include_dir;

#[cfg(feature = "includeui")]
const PROJECT_DIR: include_dir::Dir = include_dir!("ui");// /empty

struct FileWrapper {
    pub path: PathBuf,
    pub contents: Vec<u8>,
}

impl FileWrapper {
    #[cfg(feature = "includeui")]
    pub fn from_included(file: &include_dir::File) -> FileWrapper {
        Self {
            path: PathBuf::from(file.path),
            contents: file.contents().to_vec(),
        }
    }
    pub fn from_filesystem(path: &str) -> Option<FileWrapper> {
        use std::fs;
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let file = Path::new(manifest_dir).join("ui").join(path);
        fs::read(&file).ok().and_then(|buf| Some(FileWrapper {
            path: file,
            contents: buf,
        }))
    }
}

async fn echo(state: HttpServerStateSync, req: Request<Body>) -> Result<Response<Body>, CaptivePortalError> {
    let mut response = Response::new(Body::empty());

    if req.method() == Method::GET {
        if req.uri().path() == "/networks" {
            let state = state.lock().unwrap();
            let data = serde_json::to_string(&state.connections)?;
            response.headers_mut().append("Content-Type", HeaderValue::from_str("application/json").unwrap());
            *response.body_mut() = Body::from(data);
            return Ok(response);
        }

        let path = &req.uri().path()[1..];

        let mut file = match () {
            #[cfg(not(feature = "includeui"))]
            () => FileWrapper::from_filesystem(path),
            #[cfg(feature = "includeui")]
            () => PROJECT_DIR.get_file(path).and_then(|f| Some(FileWrapper::from_included(&f))),
        };
        // A captive portal catches all GET requests (that accept */* or text) and redirects to the main page.
        if file.is_none() {
            if let Some(v) = req.headers().get("Accept") {
                let accept = v.to_str().unwrap();
                if accept.contains("text") || accept.contains("*/*") {
                    let state = state.lock().unwrap();
                    let redirect_loc = format!("http://{}:{}/index.html", state.server_ip.to_string(), state.port);
                    *response.status_mut() = StatusCode::FOUND;
                    response.headers_mut().append("Location", HeaderValue::from_str(&redirect_loc).unwrap());
                    return Ok(response);
                }
            }
        }

        // Serve UI
        if let Some(file) = file {
            let mime = match file.path.extension() {
                Some(ext) => {
                    match mime_guess::from_ext(ext.to_str().unwrap()).first() {
                        Some(v) => v.to_string(),
                        None => "application/octet-stream".to_owned()
                    }
                }
                None => "application/octet-stream".to_owned()
            };
            info!("Serve {} for {}", mime, path);
            response.headers_mut().append("Content-Type", HeaderValue::from_str(&mime).unwrap());
            *response.body_mut() = Body::from(file.contents);
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
        let sender = { // unlock mutex as soon as possible
            let mut state = state.lock().unwrap();
            state.connection_sender.take().unwrap()
        };
        sender.send(Some(parsed))
            .map_err(|_| CaptivePortalError::Generic("Failed to internally route data"))?;
        return Ok(response);
    }

    *response.status_mut() = StatusCode::NOT_FOUND;
    Ok(response)
}

impl HttpServer {
    pub fn into(self) ->
    (tokio::sync::oneshot::Receiver<()>,
     tokio::sync::oneshot::Receiver<Option<WifiConnectionRequest>>,
     HttpServerStateSync, Ipv4Addr, u16) {
        (self.exit_handler, self.connection_receiver, self.state, self.server_ip, self.port)
    }
}

impl HttpServer {
    pub fn new(server_ip: Ipv4Addr, port: u16) -> (tokio::sync::oneshot::Sender<()>,
                                                   HttpServer) {
        let (tx, exit_handler) = tokio::sync::oneshot::channel::<()>();
        let (connection_sender, connection_receiver) = tokio::sync::oneshot::channel::<Option<WifiConnectionRequest>>();

        (tx, HttpServer {
            exit_handler,
            connection_receiver,
            server_ip: server_ip.clone(),
            port,
            state: Arc::new(Mutex::new(HttpServerState {
                connection_sender: Some(connection_sender),
                connections: WifiConnections(Vec::new()),
                server_ip,
                port,
                dest: None,
            })),
        })
    }

    /// Consumes the server object and runs it until it receives an exit signal via
    /// the [`tokio::sync::oneshot::Sender`] returned by [`new`]. Also quits the server
    /// when
    pub async fn run(self: HttpServer,
    ) -> Result<Option<WifiConnectionRequest>, super::CaptivePortalError> {
        let (exit_handler, connection_receiver, state, server_ip, port) = self.into();

        let state2 = state.clone();
        let make_service = make_service_fn(move |socket: &AddrStream| {
            let _remote_addr = socket.remote_addr();
            let state = state.clone();
            async move {
                let fun = service_fn(move |req| {
                    echo(state.clone(), req)
                });
                Ok::<_, hyper::Error>(fun)
            }
        });

        use futures_util::try_future::TryFutureExt;

        let addr = (server_ip.octets(), port).into();
        let server = Server::bind(&addr).serve(make_service);

        let state = state2.clone();
        let graceful = server.with_graceful_shutdown(async move {
            let r = try_select(exit_handler, connection_receiver).await.ok().unwrap();
            match r {
                Either::Right(f) => {
                    let mut state2 = state2.lock().unwrap();
                    state2.dest = f.0;
                    info!("Received connect state {:?}", state2.dest);
                }
                _ => ()
            };
            ()
        });
        graceful.await?;
        let mut state = state.lock().unwrap();
        Ok(state.dest.take())
    }
}