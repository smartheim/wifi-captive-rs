//! Placeholder until the dbus crate has proper async support.
//!
//! This module offers a background IOResource future that must be scheduled on an executor.
//! It also provides the SignalStream type, a convenience stream type that allows to listen
//! to dbus signals as a future stream.

use dbus::channel::{BusType, Channel, MatchingReceiver};
use dbus::nonblock::{LocalConnection, Process, SyncConnection};
use dbus::Error;

use std::sync::{Arc, Mutex};
use std::{future, pin, task};

use dbus::message::{MatchRule, SignalArgs};
use futures_core::task::Poll;
use std::collections::VecDeque;
use std::os::unix::io::FromRawFd;
use std::pin::Pin;
use std::task::Waker;
use tokio::stream::Stream;
use tokio_net::util::PollEvented;

use std::sync::atomic::{AtomicUsize, Ordering};
//use mio::Registration;

/// The I/O Resource should be spawned onto a Tokio compatible reactor.
///
/// If you need to ever cancel this resource (i e disconnect from D-Bus),
/// you need to make this future abortable. If it finishes, you probably lost
/// contact with the D-Bus server.
pub struct IOResource<C> {
    connection: Arc<C>,
    io: PollEvented<mio_uds::UnixStream>,
}

impl<C: AsRef<Channel> + Process> IOResource<C> {
    fn poll_internal(
        &self,
        ctx: &mut task::Context<'_>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Waker for writes
        self.connection.set_waker(ctx.waker().clone());

        let c: &Channel = (*self.connection).as_ref();

        let mut has_flushed = false;
        const TIMEOUT_SECS: std::time::Duration = std::time::Duration::from_secs(5);
        while let Poll::Ready(_) = self.io.poll_read_ready(ctx, mio::Ready::readable())? {
            has_flushed = true;
            c.read_write(Some(TIMEOUT_SECS))
                .map_err(|_| Error::new_failed("Read/write failed"))?;
            self.connection.process_all();
            //println!("read/write");
        }

        if !has_flushed {
            c.read_write(Some(TIMEOUT_SECS))
                .map_err(|_| Error::new_failed("Read/write failed"))?;
            self.connection.process_all();
            //println!("flush");
        }

        self.connection.drops(ctx);
        Ok(())
    }
}

impl<C: AsRef<Channel> + Process> future::Future for IOResource<C> {
    type Output = Box<dyn std::error::Error + Send + Sync>;
    fn poll(self: pin::Pin<&mut Self>, ctx: &mut task::Context<'_>) -> task::Poll<Self::Output> {
        match self.poll_internal(ctx) {
            Ok(_) => task::Poll::Pending,
            Err(e) => task::Poll::Ready(e),
        }
    }
}

/// Generic connection creator, you might want to use e g `new_session_local`, `new_system_sync` etc for convenience.
pub fn new<C: From<Channel>>(b: BusType) -> Result<(IOResource<C>, Arc<C>), Error> {
    let mut channel = Channel::get_private(b)?;
    channel.set_watch_enabled(true);
    let w = channel.watch();

    let conn = Arc::new(C::from(channel));
    let res = IOResource {
        connection: conn.clone(),
        io: PollEvented::new(unsafe { mio_uds::UnixStream::from_raw_fd(w.fd) }),
    };

    Ok((res, conn))
}

#[allow(dead_code)]
pub fn new_session_local() -> Result<(IOResource<LocalConnection>, Arc<LocalConnection>), Error> {
    new(BusType::Session)
}

#[allow(dead_code)]
pub fn new_system_local() -> Result<(IOResource<LocalConnection>, Arc<LocalConnection>), Error> {
    new(BusType::System)
}

#[allow(dead_code)]
pub fn new_session_sync() -> Result<(IOResource<SyncConnection>, Arc<SyncConnection>), Error> {
    new(BusType::Session)
}

pub fn new_system_sync() -> Result<(IOResource<SyncConnection>, Arc<SyncConnection>), Error> {
    new(BusType::System)
}

static GLOBAL: AtomicUsize = AtomicUsize::new(1);

struct SignalStreamState<U, T> {
    signal_queue: VecDeque<dbus::Message>,
    waker: Option<Waker>,
    mapper: Box<dyn Fn(U) -> T + Send + 'static>,
}

/// The signal stream type handles the signal registration process and offers a convenience interface
/// over the connections *start_receive* and *stop_receive* method.
pub struct SignalStream<U, T> {
    connection: Arc<SyncConnection>,
    rule_handler: u32,
    state: Arc<Mutex<SignalStreamState<U, T>>>,
    stream_id: usize,
}

impl<U: SignalArgs + 'static, T: Sized + 'static> Stream for SignalStream<U, T>
where
    U: dbus::arg::ReadAll,
{
    type Item = (T, String);
    fn poll_next(self: Pin<&mut Self>, ctx: &mut task::Context) -> task::Poll<Option<Self::Item>> {
        let mut state = self.state.lock().expect("Unlock mutex stream state");

        if let Some(message) = state.signal_queue.pop_back() {
            let v = U::from_message(&message).unwrap();
            let v = (state.mapper)(v);
            return task::Poll::Ready(Some((
                v,
                message
                    .path()
                    .and_then(|f| Some(f.to_string()))
                    .unwrap_or_default(),
            )));
        }
        state.waker = Some(ctx.waker().clone());
        task::Poll::Pending
    }
}

impl<U: SignalArgs + 'static, T: Sized + 'static> SignalStream<U, T> {
    /// Create a new signal stream. This works with [`SyncConnection`] only. Create a match rule
    /// yourself or use the dbus crate [`dbus::nonblock::Proxy`] and generated interface modules.
    pub async fn new(
        connection: Arc<SyncConnection>,
        mr: MatchRule<'static>,
        mapper: Box<dyn Fn(U) -> T + Send + 'static>,
    ) -> Result<Self, Error> {
        let match_str = mr.match_str();

        let p = dbus::nonblock::Proxy::new("org.freedesktop.DBus", "/", connection.clone());
        use dbus::nonblock::stdintf::org_freedesktop_dbus::DBus;
        p.add_match(&match_str).await?;

        let state = Arc::new(Mutex::new(SignalStreamState {
            signal_queue: Default::default(),
            waker: None,
            mapper,
        }));
        let state_clone = state.clone();
        let rule_handler = connection.start_receive(
            mr,
            Box::new(move |h: dbus::Message, _| {
                let mut state = state_clone.lock().expect("Unlock mutex stream state");
                state.signal_queue.push_front(h);
                if let Some(waker) = state.waker.take() {
                    waker.wake();
                }
                true
            }),
        );

        let stream_id = GLOBAL.fetch_add(1, Ordering::SeqCst);
        info!("Create stream {} - {} ...", stream_id, &match_str);
        Ok(SignalStream {
            connection,
            rule_handler,
            state,
            stream_id,
        })
    }
}

/// Remove the receive dispatcher rule and then ask the dbus daemon to no longer send us messages
/// of this match_rule.
impl<U, T> Drop for SignalStream<U, T> {
    fn drop(&mut self) {
        self.rule_handler = 0;
        let stream_id = self.stream_id;
        self.connection.stop_receive(self.rule_handler);

        info!("Drop stream {}...", stream_id);
    }
}
