use dbus::channel::{Channel, BusType, MatchingReceiver};
use dbus::nonblock::{LocalConnection, SyncConnection, Process};
use dbus::Error;

use std::{future, task, pin};
use std::sync::{Arc, Mutex};

use tokio::spawn;
use tokio_net::driver::Registration;

use std::pin::Pin;
use dbus::message::{SignalArgs, MatchRule};
use std::collections::VecDeque;
use std::task::Waker;
use tokio::stream::Stream;

/// The I/O Resource should be spawned onto a Tokio compatible reactor.
///
/// If you need to ever cancel this resource (i e disconnect from D-Bus),
/// you need to make this future abortable. If it finishes, you probably lost
/// contact with the D-Bus server.
pub struct IOResource<C> {
    connection: Arc<C>,
    registration: Registration,
}

impl<C: AsRef<Channel> + Process> IOResource<C> {
    fn poll_internal(&self, _ctx: &mut task::Context<'_>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let c: &Channel = (*self.connection).as_ref();
        let w = c.watch();
        let r = &self.registration;
        r.register(&mio::unix::EventedFd(&w.fd))?;
        r.take_read_ready()?;
        r.take_write_ready()?;

        if w.read { let _ = r.poll_read_ready(_ctx)?; };
        if w.write { let _ = r.poll_write_ready(_ctx)?; };

        c.read_write(Some(Default::default())).map_err(|_| Error::new_failed("Read/write failed"))?;
        self.connection.process_all();
        Ok(())
    }
}

impl<C: AsRef<Channel> + Process> future::Future for IOResource<C> {
    fn poll(self: pin::Pin<&mut Self>, ctx: &mut task::Context<'_>) -> task::Poll<Self::Output> {
        match self.poll_internal(ctx) {
            Ok(_) => {
                task::Poll::Pending
            }
            Err(e) => task::Poll::Ready(e),
        }
    }
    type Output = Box<dyn std::error::Error + Send + Sync>;
}


/// Generic connection creator, you might want to use e g `new_session_local`, `new_system_sync` etc for convenience.
pub fn new<C: From<Channel>>(b: BusType) -> Result<(IOResource<C>, Arc<C>), Error> {
    let mut channel = Channel::get_private(b)?;
    channel.set_watch_enabled(true);

    let conn = Arc::new(C::from(channel));
    let res = IOResource { connection: conn.clone(), registration: Registration::new() };
    Ok((res, conn))
}

pub fn new_session_local() -> Result<(IOResource<LocalConnection>, Arc<LocalConnection>), Error> { new(BusType::Session) }

pub fn new_system_local() -> Result<(IOResource<LocalConnection>, Arc<LocalConnection>), Error> { new(BusType::System) }

pub fn new_session_sync() -> Result<(IOResource<SyncConnection>, Arc<SyncConnection>), Error> { new(BusType::Session) }

pub fn new_system_sync() -> Result<(IOResource<SyncConnection>, Arc<SyncConnection>), Error> { new(BusType::System) }

struct SignalStreamState<U, T> {
    signal_queue: VecDeque<dbus::Message>,
    waker: Option<Waker>,
    done: bool,
    mapper: Box<dyn Fn(U) -> T+ Send+ 'static>,
}


pub struct SignalStream<U, T> {
    connection: Arc<SyncConnection>,
    match_str: String,
    rule_handler: u32,
    state: Arc<Mutex<SignalStreamState<U, T>>>,
}

impl<U: SignalArgs + 'static, T: Sized+ 'static> Stream for SignalStream<U, T>
    where U: dbus::arg::ReadAll {
    type Item = (T, String);
    fn poll_next(self: Pin<&mut Self>, ctx: &mut task::Context) -> task::Poll<Option<Self::Item>> {
        let mut state = self.state.lock().unwrap();

        if let Some(message) = state.signal_queue.pop_back() {
            let v = U::from_message(&message).unwrap();
            let v = (state.mapper)(v);
            return task::Poll::Ready(Some((v, message.path().and_then(|f| Some(f.to_string())).unwrap_or_default())));
        }
        if state.done {
            return task::Poll::Ready(None);
        }
        state.waker = Some(ctx.waker().clone());
        task::Poll::Pending
    }
}

impl<U: SignalArgs + 'static, T: Sized+ 'static> SignalStream<U, T> {
    pub async fn new(connection: Arc<SyncConnection>, mr: MatchRule<'static>,
                     mapper: Box<dyn Fn(U) -> T + Send + 'static>) -> Result<Self, Error> {
        let match_str = mr.match_str();

        let p = dbus::nonblock::Proxy::new("org.freedesktop.DBus", "/", connection.clone());
        use dbus::nonblock::stdintf::org_freedesktop_dbus::DBus;
        p.add_match(&match_str).await?;

        let state = Arc::new(Mutex::new(SignalStreamState { signal_queue: Default::default(), waker: None, done: false, mapper }));
        let state_clone = state.clone();
        let rule_handler = connection.start_receive(
            mr,
            Box::new(move |h: dbus::Message, _| {
                let mut state = state_clone.lock().unwrap();
                state.signal_queue.push_front(h);
                if let Some(waker) = state.waker.take() {
                    waker.wake();
                }
                true
            }),
        );

        Ok(SignalStream { connection, match_str, rule_handler, state })
    }
}

/// Remove the receive dispatcher rule and then ask the dbus daemon to no longer send us messages
/// of this match_rule.
impl<U, T> Drop for SignalStream<U, T> {
    fn drop(&mut self) {
        self.connection.stop_receive(self.rule_handler);
        self.rule_handler = 0;
        {
            let mut state = self.state.lock().unwrap();
            state.done = true;
        }
        let match_str = self.match_str.clone();
        let connection = self.connection.clone();
        spawn(async move {
            let p = dbus::nonblock::Proxy::new("org.freedesktop.DBus", "/", connection);
            use dbus::nonblock::stdintf::org_freedesktop_dbus::DBus;
            let _ = p.remove_match(&match_str).await;
        });
    }
}
