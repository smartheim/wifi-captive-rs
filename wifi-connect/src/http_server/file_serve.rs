//! Serves the static ui files. If the "includeui" feature is set, the ui files are compiled in
//! and no system file access is required.

use super::CaptivePortalError;
use crate::http_server::HttpServerStateSync;
use hyper::header::HeaderValue;
use hyper::{Body, Request, Response, StatusCode};
use std::path::{Path, PathBuf};

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

impl<'a> FileWrapper {
    #[cfg(feature = "includeui")]
    pub fn from_included(file: &include_dir::File) -> FileWrapper {
        Self {
            path: PathBuf::from(file.path),
            contents: Vec::with_capacity(0),
            embedded_file: Some(file.clone()),
        }
    }

    #[cfg(not(feature = "includeui"))]
    pub fn from_filesystem(root: &Path, path: &str) -> Option<FileWrapper> {
        use std::fs;
        let file = root.join("ui").join(path);
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

pub fn serve_file(
    root: &Path,
    mut response: Response<Body>,
    req: &Request<Body>,
    state: &HttpServerStateSync,
) -> Result<Response<Body>, CaptivePortalError> {
    let path = &req.uri().path()[1..];

    let file = match () {
        #[cfg(not(feature = "includeui"))]
        () => FileWrapper::from_filesystem(root, path),
        #[cfg(feature = "includeui")]
        () => PROJECT_DIR
            .get_file(path)
            .and_then(|f| Some(FileWrapper::from_included(&f))),
    };
    // A captive portal catches all GET requests (that accept */* or text) and redirects to the main page.
    if file.is_none() {
        if let Some(v) = req.headers().get("Accept") {
            let accept = v.to_str()?;
            if accept.contains("text") || accept.contains("*/*") {
                let state = state.lock().expect("Lock http_state mutex");
                let redirect_loc = format!(
                    "http://{}:{}/index.html",
                    state.server_addr.ip().to_string(),
                    state.server_addr.port()
                );
                drop(state); // release mutex
                *response.status_mut() = StatusCode::FOUND;
                response.headers_mut().append(
                    "Location",
                    HeaderValue::from_str(&redirect_loc)
                        .expect("Headervalue from generated string"),
                );
                return Ok(response);
            }
        }
    }

    // Serve UI
    if let Some(file) = file {
        let mime = match file.path().extension() {
            Some(ext) => {
                match mime_guess::from_ext(ext.to_str().expect("file path extension OsStr->str"))
                    .first()
                {
                    Some(v) => v.to_string(),
                    None => "application/octet-stream".to_owned(),
                }
            },
            None => "application/octet-stream".to_owned(),
        };
        info!("Serve {} for {}", mime, path);
        response.headers_mut().append(
            "Content-Type",
            HeaderValue::from_str(&mime).expect("mime to header value"),
        );
        *response.body_mut() = file.contents();
        return Ok(response);
    }

    *response.status_mut() = StatusCode::NOT_FOUND;
    Ok(response)
}
