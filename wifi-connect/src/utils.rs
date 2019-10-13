use super::errors::CaptivePortalError;
use ascii::AsciiStr;

use futures_util::future::Either;
use futures_util::try_future::try_select;
use futures_util::StreamExt;
use pin_utils::pin_mut;
use std::net::SocketAddr;
use std::time::Duration;
use tokio_net::udp::UdpSocket;

/// A wifi password set by this service can only contain ASCII characters. Just to be sure.
pub(crate) fn verify_ascii_password(password: String) -> Result<String, CaptivePortalError> {
    match AsciiStr::from_ascii(&password) {
        Err(_e) => Err(CaptivePortalError::invalid_shared_key(
            "Not an ASCII password".into(),
        )),
        Ok(p) => {
            if p.len() < 8 {
                Err(CaptivePortalError::invalid_shared_key(format!(
                    "Password length should be at least 8 characters: {} len",
                    p.len()
                )))
            } else if p.len() > 32 {
                Err(CaptivePortalError::invalid_shared_key(format!(
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
            // Server exit handler dropped
            Either::Right((_, _)) => Ok(None),
        },
    }
}

/// Wraps the given future with a ctrl+c signal listener. Returns None if the signal got caught
/// and Some(return_value) otherwise.
pub async fn ctrl_c_or_future<F, R>(connect_future: F) -> Result<Option<R>, CaptivePortalError>
where
    F: std::future::Future<Output = Result<R, CaptivePortalError>>,
    R: Sized,
{
    let ctrl_c = async move {
        match tokio_net::signal::ctrl_c() {
            Ok(mut v) => {
                v.next().await;
                Ok(())
            },
            Err(_) => Err(CaptivePortalError::Generic("signal::ctrl_c() failed")),
        }
    };
    pin_utils::pin_mut!(ctrl_c);
    pin_utils::pin_mut!(connect_future);

    let r = try_select(connect_future, ctrl_c).await;
    match r {
        Err(e) => {
            if let Either::Left((e, _)) = e {
                return Err(e);
            }
        },
        Ok(v) => {
            if let Either::Left((v, _)) = v {
                return Ok(Some(v));
            }
        },
    }

    Ok(None)
}

/// Wraps the given future with a timeout. Returns None if the timeout happened before the given
/// future resolved and Some(return_value) otherwise.
pub async fn timed_future<F, R>(connect_future: F, duration: Duration) -> Option<R>
where
    F: std::future::Future<Output = R>,
    R: Sized,
{
    let timed_future = tokio_timer::delay_for(duration);
    pin_utils::pin_mut!(timed_future);
    pin_utils::pin_mut!(connect_future);

    let r = futures_util::future::select(connect_future, timed_future).await;
    if let Either::Left((v, _)) = r {
        return Some(v);
    }

    None
}
