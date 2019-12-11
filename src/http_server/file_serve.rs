//! Serves the static ui files. If the "includeui" feature is set, the ui files are compiled in
//! and no system file access is required.

use super::CaptivePortalError;
use crate::http_server::HttpServerStateSync;
use hyper::header::HeaderValue;
use hyper::{Body, Request, Response, StatusCode};
use std::path::{Path, PathBuf};

#[cfg(any(feature = "includeui", not(debug_assertions)))]
use include_dir::{include_dir};

#[cfg(any(feature = "includeui", not(debug_assertions)))]
/// A reference to all binary embedded ui files
const PROJECT_DIR: include_dir::Dir = include_dir!("ui");

/// The file wrapper struct deals with the fact that we either read a file from the filesystem
/// or use a binary embedded variant. That means we either allocate a vector for the file content,
/// or use a pointer to the data without any allocation.
#[cfg(any(feature = "includeui", not(debug_assertions)))]
struct FileWrapper {
    path: PathBuf,
    contents: &'static [u8],
}

#[cfg(all(not(feature = "includeui"), debug_assertions))]
struct FileWrapper {
    path: PathBuf,
    contents: Vec<u8>,
}

struct R<'a>(&'a [u8]);
unsafe fn extend_lifetime<'b>(r: R<'b>) -> R<'static> {
    std::mem::transmute::<R<'b>, R<'static>>(r)
}

#[cfg(any(feature = "includeui", not(debug_assertions)))]
impl<'a> FileWrapper {
    pub fn from_included(file: &'a include_dir::File) -> FileWrapper {
        Self {
            path: PathBuf::from(file.path),
            // This is safe, because the author of the include_dir himself wrote in
            // the documentation: "A file with its contents stored in a &'static [u8]"
            contents: unsafe { extend_lifetime(R(file.contents())) }.0,
        }
    }

    pub fn path(&'a self) -> &'a Path {
        &self.path
    }

    /// The file's raw contents.
    /// This method consumes the file wrapper
    pub fn contents(self) -> Body {
        Body::from(self.contents)
    }
}

#[cfg(all(not(feature = "includeui"), debug_assertions))]
impl<'a> FileWrapper {
    pub fn from_filesystem(root: &Path, path: &str) -> Option<FileWrapper> {
        use std::fs;
        let file = root.join("ui").join(path);
        fs::read(&file).ok().and_then(|buf| {
            Some(FileWrapper {
                path: file,
                contents: buf,
            })
        })
    }

    pub fn path(&'a self) -> &'a Path {
        &self.path
    }

    /// The file's raw contents.
    /// This method consumes the file wrapper
    pub fn contents(self) -> Body {
        Body::from(self.contents)
    }
}

fn mime_type_from_ext(ext: &str) -> &str {
    match ext {
        "html" => "text/html",
        "js" => "application/javascript",
        "png" => "image/png",
        "css" => "text/css",
        _ => "application/octet-stream",
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
        #[cfg(all(not(feature = "includeui"), debug_assertions))]
        () => FileWrapper::from_filesystem(root, path),
        #[cfg(any(feature = "includeui", not(debug_assertions)))]
        () => {
            drop(root);
            PROJECT_DIR
                .get_file(path)
                .and_then(|f| Some(FileWrapper::from_included(&f)))
        },
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
                    HeaderValue::from_str(&redirect_loc).expect("Headervalue from generated string"),
                );
                return Ok(response);
            }
        }
    }

    // Serve UI
    if let Some(file) = file {
        let mime = match file.path().extension() {
            Some(ext) => mime_type_from_ext(ext.to_str().expect("file path extension OsStr->str")),
            None => "application/octet-stream",
        };
        info!("Serve {} for {}", mime, path);
        response.headers_mut().append(
            "Content-Type",
            HeaderValue::from_str(mime).expect("mime to header value"),
        );
        *response.body_mut() = file.contents();
        return Ok(response);
    }

    *response.status_mut() = StatusCode::NOT_FOUND;
    Ok(response)
}
