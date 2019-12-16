//! # A convenience stream type that allows to listen to dbus signals as a future stream

use dbus::channel::{MatchingReceiver, Token};
use dbus::nonblock::SyncConnection;
use dbus::Error;

use std::sync::{Arc, Mutex};
use std::task;

use dbus::message::{MatchRule, SignalArgs};
use std::collections::VecDeque;
use std::pin::Pin;
use std::task::Waker;

use serde::export::PhantomData;
use futures_core::Stream;

struct SignalStreamState<U> {
    signal_queue: VecDeque<dbus::Message>,
    waker: Option<Waker>,
    rule_handler: Token,
    _u: PhantomData<U>,
}

/// The signal stream type handles the signal registration process and offers a convenience interface
/// over the connections *start_receive* and *stop_receive* method.
pub struct SignalStream<U> {
    connection: Arc<SyncConnection>,
    rule_handler: Token,
    state: Arc<Mutex<SignalStreamState<U>>>,
}

impl<U: SignalArgs + 'static> Stream for SignalStream<U>
where
    U: dbus::arg::ReadAll,
{
    type Item = (U, String);
    fn poll_next(self: Pin<&mut Self>, ctx: &mut task::Context) -> task::Poll<Option<Self::Item>> {
        let mut state = self.state.lock().expect("Unlock mutex stream state");

        debug!(
            "Wake up stream {}, queue: {}",
            self.rule_handler.0,
            state.signal_queue.len()
        );
        if let Some(message) = state.signal_queue.pop_back() {
            let v = U::from_message(&message);
            match v {
                None => {
                    warn!(
                        "Unexpected message on {},{:?}",
                        message.path().and_then(|f| Some(f.to_string())).unwrap_or_default(),
                        message
                    );
                    return task::Poll::Ready(None);
                },
                Some(v) => {
                    return task::Poll::Ready(Some((
                        v,
                        message.path().and_then(|f| Some(f.to_string())).unwrap_or_default(),
                    )));
                },
            }
        }
        if state.waker.is_none() {
            state.waker = Some(ctx.waker().clone());
        }
        task::Poll::Pending
    }
}

impl<U: SignalArgs + 'static + Send> SignalStream<U> {
    /// Create a new signal stream. This works with [`SyncConnection`] only.
    ///
    /// Create a match rule like this:
    /// `let mr = MatchRule::new_signal("com.example.dbustest", "HelloHappened");`
    pub async fn new(connection: Arc<SyncConnection>, mr: MatchRule<'static>) -> Result<Self, Error> {
        let match_str = mr.match_str();

        let p = dbus::nonblock::Proxy::new("org.freedesktop.DBus", "/org/freedesktop/DBus", connection.clone());
        use dbus::nonblock::stdintf::org_freedesktop_dbus::DBus;
        p.add_match(&match_str).await?;

        let state = Arc::new(Mutex::new(SignalStreamState {
            signal_queue: Default::default(),
            waker: None,
            rule_handler: Token(0),
            _u: PhantomData,
        }));
        let state_clone = state.clone();
        let rule_handler = connection.start_receive(
            mr,
            Box::new(move |h: dbus::Message, _| {
                let waker = {
                    let mut state = state_clone.lock().expect("Unlock mutex stream state");
                    state.signal_queue.push_front(h);
                    debug!(
                        "Add to stream {}, queue: {}",
                        state.rule_handler.0,
                        state.signal_queue.len()
                    );
                    state.waker.clone()
                };
                if let Some(ref waker) = waker {
                    waker.wake_by_ref();
                }
                true
            }),
        );

        {
            let mut state = state.lock().expect("Unlock mutex stream state");
            state.rule_handler = rule_handler;
        }

        debug!("Create stream {} - {} ...", rule_handler.0, &match_str);
        Ok(SignalStream {
            connection,
            rule_handler,
            state,
        })
    }

    /// Create a new signal stream. This works with [`SyncConnection`] only.
    ///
    /// This is a convenience function for streams that operate on org.freedesktop.DBus.Properties changes.
    pub async fn prop_new(
        wifi_device_path: &dbus::Path<'_>,
        conn: Arc<SyncConnection>,
    ) -> Result<SignalStream<U>, Error> {
        let rule = U::match_rule(None, Some(wifi_device_path)).static_clone();
        Ok(SignalStream::new(conn, rule).await?)
    }
}

/// Remove the receive dispatcher rule and then ask the dbus daemon to no longer send us messages
/// of this match_rule.
impl<U> Drop for SignalStream<U> {
    fn drop(&mut self) {
        self.connection.stop_receive(self.rule_handler);
        debug!("Drop stream {}...", self.rule_handler.0);
    }
}
