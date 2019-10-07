use super::errors::CaptivePortalError;
use ascii::AsciiStr;

use futures_util::future::Either;
use futures_util::try_future::try_select;
use pin_utils::pin_mut;
use std::net::SocketAddr;
use tokio_net::udp::UdpSocket;

/// A wifi password set by this service can only contain ASCII characters. Just to be sure.
pub(crate) fn verify_ascii_password(password: String) -> Result<String, CaptivePortalError> {
    match AsciiStr::from_ascii(&password) {
        Err(_e) => Err(CaptivePortalError::pre_shared_key(
            "Not an ASCII password".into(),
        )),
        Ok(p) => {
            if p.len() < 8 {
                Err(CaptivePortalError::pre_shared_key(format!(
                    "Password length should be at least 8 characters: {} len",
                    p.len()
                )))
            } else if p.len() > 32 {
                Err(CaptivePortalError::pre_shared_key(format!(
                    "Password length should not exceed 64: {} len",
                    p.len()
                )))
            } else {
                Ok(password)
            }
        },
    }
}

/// Receives the next packet on a udp socket. The future resolves if either a packet got received,
/// an error occurred or the exit handler that belongs to the given exit_receiver has been triggered.
pub async fn receive_or_exit(
    socket: &mut UdpSocket,
    exit_receiver: &mut tokio::sync::oneshot::Receiver<()>,
    in_buf: &mut [u8],
) -> Result<Option<(usize, SocketAddr)>, CaptivePortalError> {
    // The receive future will be wrapped in a try_select. pin it.
    let receive_future = socket.recv_from(in_buf);
    pin_mut!(receive_future);
    pin_mut!(exit_receiver);

    // Create a future that resolves if either of two futures resolve (receive, exit handler)
    let future = try_select(receive_future, exit_receiver);

    match future.await {
        Ok(v) => {
            if let Either::Left(((size, socket_addr), _)) = v {
                Ok(Some((size, socket_addr)))
            } else {
                Ok(None)
            }
        },
        Err(e) => match e {
            Either::Left((e, _)) => Err(CaptivePortalError::IO(e)),
            Either::Right((_, _)) => Err(CaptivePortalError::Generic(
                "Server exit handler dropped! This is a bug!",
            )),
        },
    }
}
