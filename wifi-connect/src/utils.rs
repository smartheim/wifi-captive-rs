use super::CaptivePortalError;
use ascii::AsciiStr;

use futures_util::future::Either;
use futures_util::stream::Stream;
use futures_util::try_future::try_select;
use futures_util::StreamExt;
use pin_utils::pin_mut;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{self, Poll};
use std::time::Duration;
use tokio::timer::Delay;
use tokio_net::signal::CtrlC;
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

    info!("SIGKILL: Graceful shutdown initialized ...");
    Ok(None)
}

pub struct CtrlCSignal<T> {
    value: T,
    sig: CtrlC,
    exit_handler: Option<tokio::sync::oneshot::Sender<()>>,
}

impl<T: Future> CtrlCSignal<T> {
    pub fn new(value: T, exit_handler: tokio::sync::oneshot::Sender<()>) -> CtrlCSignal<T> {
        let sig = tokio_net::signal::ctrl_c().expect("Ctrl+C signal handler");
        CtrlCSignal {
            value,
            sig,
            exit_handler: Some(exit_handler),
        }
    }
}

impl<T: Future> Future for CtrlCSignal<T> {
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
            let sig = unsafe { self.as_mut().map_unchecked_mut(|me| &mut me.sig) };
            if let Poll::Ready(option) = sig.poll_next(cx) {
                if let Some(_) = option {
                    let mut exit_handler_option =
                        unsafe { self.as_mut().map_unchecked_mut(|me| &mut me.exit_handler) };
                    // unwrap is safe, because of wrapping is_some.
                    let _ = exit_handler_option.take().unwrap().send(());
                }
            }
        }
        Poll::Pending
    }
}

pub trait FutureWithSignalCancel: Future {
    fn ctrl_c(self, exit_handler: tokio::sync::oneshot::Sender<()>) -> CtrlCSignal<Self>
    where
        Self: Sized,
    {
        CtrlCSignal::new(self, exit_handler)
    }
}

impl<T: ?Sized> FutureWithSignalCancel for T where T: Future {}

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
        let delay = tokio_timer::delay_for(timeout);
        Timeout {
            value: self,
            delay,
            exit_handler: Some(exit_handler),
        }
    }
}

impl<T: ?Sized> FutureWithTimeout for T where T: Future {}
