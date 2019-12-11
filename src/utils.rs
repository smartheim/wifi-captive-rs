//! # Utility methods and types
use super::CaptivePortalError;

use futures_util::future::Either;
use futures_util::future::try_select;
use pin_utils::pin_mut;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{self, Poll};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::Delay;
use tokio::signal::ctrl_c;

/// A wifi password must be between 8 and 32 characters
pub fn verify_password(password: &str) -> Result<(), CaptivePortalError> {
    if password.len() < 8 {
        Err(CaptivePortalError::InvalidSharedKey(format!(
            "Password length should be at least 8 characters: {} len",
            password.len()
        )))
    } else if password.len() > 32 {
        Err(CaptivePortalError::InvalidSharedKey(format!(
            "Password length should not exceed 64: {} len",
            password.len()
        )))
    } else {
        Ok(())
    }
}

/// Takes an optional field member of the portal and sets the optional to None.
///
/// Safety: Because the optional fields are never moved, this is considered safe, albeit the pinning.
pub(crate) fn take_optional<F, X, S>(mut subject: Pin<&mut S>, fun: F)
    where
        F: for<'r> FnOnce(&'r mut S) -> &'r mut Option<X>,
        X: Unpin,
{
    // Safety: we never move `self.value` (the Optional)
    let field = unsafe { subject.as_mut().map_unchecked_mut(fun) };
    // Remove future out of optional
    let _ = field.get_mut().take();
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
        }
        Err(e) => match e {
            Either::Left((e, _)) => Err(CaptivePortalError::IO(e, "Failed to receive")),
            // Server exit handler dropped
            Either::Right((_, _)) => Ok(None),
        },
    }
}

/// Wraps the given future with a ctrl+c signal listener. Returns None if the signal got caught
/// and Some(return_value) otherwise.
pub async fn ctrl_c_or_future<F, R>(connect_future: F) -> Result<Option<R>, CaptivePortalError>
    where
        F: std::future::Future<Output=Result<R, CaptivePortalError>>,
        R: Sized,
{
    let ctrlc = ctrl_c();
    pin_utils::pin_mut!(ctrlc);
    pin_utils::pin_mut!(connect_future);

    let r = try_select(connect_future, ctrlc).await;
    match r {
        Err(e) => {
            if let Either::Left((e, _)) = e {
                return Err(e);
            }
        }
        Ok(v) => {
            match v {
                Either::Left((v, _)) => {
                    return Ok(Some(v));
                }
                Either::Right((_, _)) => {}
            }
        }
    }

    info!("SIGKILL: Graceful shutdown initialized ...");
    Ok(None)
}


/// Wraps the given future with a ctrl+c signal listener. Returns None if the signal got caught
/// and Some(return_value) otherwise.
pub async fn ctrl_c_with_exit_handler<F, R>(connect_future: F, exit_handler: tokio::sync::oneshot::Sender<()>) -> Result<Option<R>, CaptivePortalError>
    where
        F: std::future::Future<Output=Result<R, CaptivePortalError>>,
        R: Sized,
{
    let ctrlc = ctrl_c();
    pin_utils::pin_mut!(ctrlc);
    pin_utils::pin_mut!(connect_future);

    let r = try_select(connect_future, ctrlc).await;
    match r {
        Err(e) => {
            if let Either::Left((e, _)) = e {
                return Err(e);
            }
        }
        Ok(v) => {
            match v {
                Either::Left((v, _)) => {
                    return Ok(Some(v));
                }
                Either::Right((_, w)) => {
                    // Ctrl+C invoked. Send exit signal and await future
                    exit_handler.send(()).map_err(|_e| CaptivePortalError::Generic("Failed to use ctrl_c exit handler".to_owned()))?;
                    return Ok(Some(w.await?));
                }
            }
        }
    }

    info!("SIGKILL: Graceful shutdown initialized ...");
    Ok(None)
}

/// A timeout future that calls the given exit handler on a timeout and drives the inner future to completion.
pub struct Timeout<T, DROP> {
    value: T,
    delay: Delay,
    exit_handler: Option<DROP>,
}

//impl<T: Future, DROP: Sized> Timeout<T, DROP> {
/// Triggers an early timeout
//    pub fn trigger_timeout(mut self: Pin<&mut Self>) -> Pin<&mut Self> {
//        unsafe {
//            self.as_mut().map_unchecked_mut(|me| {
//                let _ = me.exit_handler.take();
//                &mut me.exit_handler
//            })
//        };
//        self
//    }
//}

impl<T: Future, DROP: Sized> Future for Timeout<T, DROP> {
    type Output = T::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
        // First, try polling the future

        // Safety: we never move `self.value`
        let p = unsafe { self.as_mut().map_unchecked_mut(|me| &mut me.value) };
        if let Poll::Ready(v) = p.poll(cx) {
            return Poll::Ready(v);
        }

        // Now check the timer and call the exit handler if necessary
        // Safety: X_X!
        if self.exit_handler.is_some() {
            let delay = unsafe { self.as_mut().map_unchecked_mut(|me| &mut me.delay) };
            if let Poll::Ready(()) = delay.poll(cx) {
                unsafe {
                    self.as_mut().map_unchecked_mut(|me| {
                        me.exit_handler.take();
                        &mut me.exit_handler
                    });
                }
            }
        }
        Poll::Pending
    }
}

pub trait FutureWithTimeout: Future {
    fn timeout<DROP: Sized>(self, timeout: Duration, exit_handler: DROP) -> Timeout<Self, DROP>
        where
            Self: Sized,
    {
        let delay = tokio::time::delay_for(timeout);
        Timeout {
            value: self,
            delay,
            exit_handler: Some(exit_handler),
        }
    }
}

impl<T: ?Sized> FutureWithTimeout for T where T: Future {}
