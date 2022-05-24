use std::{
    collections::HashMap,
    marker::PhantomData,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, MutexGuard,
    },
};

use futures_util::{future::ready, Stream, StreamExt};
use metrics_tracing_context::MetricsLayer;
use once_cell::sync::OnceCell;
use tokio::sync::{
    broadcast::{self, Receiver, Sender},
    oneshot,
};
use tokio_stream::wrappers::BroadcastStream;
use tracing::{Event, Subscriber};
use tracing_limit::RateLimitedLayer;
use tracing_subscriber::{
    layer::{Context, SubscriberExt},
    registry::LookupSpan,
    util::SubscriberInitExt,
    Layer,
};
pub use tracing_tower::{InstrumentableService, InstrumentedService};
use value::Value;

use crate::event::LogEvent;

/// BUFFER contains all of the internal log events generated by Vector between the initialization of `tracing` and early
/// buffering being stopped, which occurs once the topology reports as having successfully started.
///
/// This means that callers must subscribe during the configuration phase of their components, and not in the core loop
/// of the component, as the topology can only report when a component has been spawned, but not necessarily always
/// when it has started doing, or waiting, for input.
static BUFFER: OnceCell<Mutex<Option<Vec<LogEvent>>>> = OnceCell::new();

/// SHOULD_BUFFER controls whether or not internal log events should be buffered or sent directly to the trace broadcast
/// channel.
static SHOULD_BUFFER: AtomicBool = AtomicBool::new(true);

/// SUBSCRIBERS contains a list of callers interested in internal log events who will be notified when early buffering
/// is disabled, by receiving a copy of all buffered internal log events.
static SUBSCRIBERS: OnceCell<Mutex<Option<Vec<oneshot::Sender<Vec<LogEvent>>>>>> = OnceCell::new();

/// SENDER holds the sender/receiver handle that will receive a copy of all the internal log events *after* the topology
/// has been initialized.
static SENDER: OnceCell<Sender<LogEvent>> = OnceCell::new();

fn metrics_layer_enabled() -> bool {
    !matches!(std::env::var("DISABLE_INTERNAL_METRICS_TRACING_INTEGRATION"), Ok(x) if x == "true")
}

pub fn init(color: bool, json: bool, levels: &str) {
    let _ = BUFFER.set(Mutex::new(Some(Vec::new())));
    let fmt_filter = tracing_subscriber::filter::Targets::from_str(levels).expect(
        "logging filter targets were not formatted correctly or did not specify a valid level",
    );

    let metrics_layer = metrics_layer_enabled()
        .then(|| MetricsLayer::new().with_filter(tracing_subscriber::filter::LevelFilter::INFO));

    let subscriber = tracing_subscriber::registry()
        .with(metrics_layer)
        .with(BroadcastLayer::new().with_filter(fmt_filter.clone()));

    #[cfg(feature = "tokio-console")]
    let subscriber = {
        let console_layer = console_subscriber::ConsoleLayer::builder()
            .with_default_env()
            .spawn();

        subscriber.with(console_layer)
    };

    if json {
        let formatter = tracing_subscriber::fmt::layer().json().flatten_event(true);

        #[cfg(test)]
        let formatter = formatter.with_test_writer();

        let rate_limited = RateLimitedLayer::new(formatter);
        let subscriber = subscriber.with(rate_limited.with_filter(fmt_filter));

        let _ = subscriber.try_init();
    } else {
        let formatter = tracing_subscriber::fmt::layer()
            .with_ansi(color)
            .with_writer(std::io::stderr);

        #[cfg(test)]
        let formatter = formatter.with_test_writer();

        let rate_limited = RateLimitedLayer::new(formatter);
        let subscriber = subscriber.with(rate_limited.with_filter(fmt_filter));

        let _ = subscriber.try_init();
    }
}

#[cfg(test)]
pub fn reset_early_buffer() -> Option<Vec<LogEvent>> {
    get_early_buffer().replace(Vec::new())
}

/// Gets a  mutable reference to the early buffer.
fn get_early_buffer() -> MutexGuard<'static, Option<Vec<LogEvent>>> {
    BUFFER
        .get()
        .expect("Internal logs buffer not initialized")
        .lock()
        .expect("Couldn't acquire lock on internal logs buffer")
}

/// Determines whether tracing events should be processed (e.g. converted to log
/// events) to avoid unnecessary performance overhead.
///
/// Checks if [`BUFFER`] is set or if a trace sender exists
fn should_process_tracing_event() -> bool {
    BUFFER.get().is_some() || maybe_get_trace_sender().is_some()
}

/// Attempts to buffer an event into the early buffer.
fn try_buffer_event(log: &LogEvent) -> bool {
    if SHOULD_BUFFER.load(Ordering::Acquire) {
        if let Some(buffer) = get_early_buffer().as_mut() {
            buffer.push(log.clone());
            return true;
        }
    }

    false
}

/// Attempts to broadcast an event to subscribers.
///
/// If no subscribers are connected, this does nothing.
fn try_broadcast_event(log: LogEvent) {
    if let Some(sender) = maybe_get_trace_sender() {
        let _ = sender.send(log);
    }
}

/// Consumes the early buffered events.
///
/// # Panics
///
/// If the early buffered events have already been consumes, this function will panic.
fn consume_early_buffer() -> Vec<LogEvent> {
    get_early_buffer()
        .take()
        .expect("early buffer was already consumed")
}

/// Gets or creates a trace sender for sending internal log events.
fn get_trace_sender() -> &'static broadcast::Sender<LogEvent> {
    SENDER.get_or_init(|| broadcast::channel(99).0)
}

/// Attempts to get the trace sender for sending internal log events.
///
/// If the trace sender has not yet been created, `None` is returned.
fn maybe_get_trace_sender() -> Option<&'static broadcast::Sender<LogEvent>> {
    SENDER.get()
}

/// Creates a trace receiver that receives internal log events.
///
/// This will create a trace sender if one did not already exist.
fn get_trace_receiver() -> broadcast::Receiver<LogEvent> {
    get_trace_sender().subscribe()
}

/// Gets a mutable reference to the list of waiting subscribers, if it exists.
fn get_trace_subscriber_list() -> MutexGuard<'static, Option<Vec<oneshot::Sender<Vec<LogEvent>>>>> {
    SUBSCRIBERS
        .get_or_init(|| Mutex::new(Some(Vec::new())))
        .lock()
        .expect("poisoned locks are dumb")
}

/// Attempts to register for early buffered events.
///
/// If early buffering has not yet been stopped, `Some(receiver)` is returned. The given receiver will resolve to a
/// vector of all early buffered events once early buffering has been stopped. Otherwise, if early buffering is already
/// stopped, `None` is returned.
fn try_register_for_early_events() -> Option<oneshot::Receiver<Vec<LogEvent>>> {
    if SHOULD_BUFFER.load(Ordering::Acquire) {
        // We're still in early buffering mode. Attempt to subscribe by adding a oneshot sender
        // to SUBSCRIBERS. If it's already been consumed, then we've gotten beaten out by a
        // caller that is disabling early buffering, so we just go with the flow either way.
        get_trace_subscriber_list().as_mut().map(|subscribers| {
            let (tx, rx) = oneshot::channel();
            subscribers.push(tx);
            rx
        })
    } else {
        // Early buffering is being or has been disabled, so we can no longer register.
        None
    }
}

/// Stops early buffering.
///
/// This flushes any buffered log events to waiting subscribers and redirects log events from the buffer to the
/// broadcast stream.
pub fn stop_early_buffering() {
    // First, consume any waiting subscribers. This causes new subscriptions to simply receive from
    // the broadcast channel, and not bother trying to receive the early buffered events.
    let subscribers = get_trace_subscriber_list().take();

    // Now that we have any waiting subscribers, actually disable early buffering and consume any
    // buffered log events. Once we have the buffered events, send them to each subscriber.
    SHOULD_BUFFER.store(false, Ordering::Release);
    let buffered_events = consume_early_buffer();
    for subscriber_tx in subscribers.into_iter().flatten() {
        // Ignore any errors sending since the caller may have dropped or something else.
        let _ = subscriber_tx.send(buffered_events.clone());
    }
}

/// A subscription to the log events flowing in via `tracing`, in the Vector native format.
///
/// Used to capture tracing events from internal log telemetry, via `tracing`, and convert them to native Vector events,
/// specifically `LogEvent`, such that they can be shuttled around and treated as normal events.  Currently only powers
/// the `internal_logs` source, but could be used for other purposes if need be.
pub struct TraceSubscription {
    buffered_events_rx: Option<oneshot::Receiver<Vec<LogEvent>>>,
    trace_rx: Receiver<LogEvent>,
}

impl TraceSubscription {
    /// Registers a subscription to the internal log event stream.
    pub fn subscribe() -> TraceSubscription {
        let buffered_events_rx = try_register_for_early_events();
        let trace_rx = get_trace_receiver();

        Self {
            buffered_events_rx,
            trace_rx,
        }
    }

    /// Gets any early buffered log events.
    ///
    /// If this subscription was registered after early buffering was turned off, `None` will be returned immediately.
    /// Otherwise, waits for early buffering to be stopped and returns `Some(events)` where `events` contains all events
    /// seen from the moment `tracing` was initialized to the moment early buffering was stopped.
    pub async fn buffered_events(&mut self) -> Option<Vec<LogEvent>> {
        // If we have a receiver for buffered events, and it returns them successfully, then pass
        // them back.  We don't care if the sender drops in the meantime, so just swallow that error.
        match self.buffered_events_rx.take() {
            Some(rx) => rx.await.ok(),
            None => None,
        }
    }

    /// Converts this subscription into a raw stream of log events.
    pub fn into_stream(self) -> impl Stream<Item = LogEvent> + Unpin {
        // We ignore errors because the only error we get is when the broadcast receiver lags, and there's nothing we
        // can actully do about that so there's no reason to force callers to even deal with it.
        BroadcastStream::new(self.trace_rx).filter_map(|event| ready(event.ok()))
    }
}

struct BroadcastLayer<S> {
    _subscriber: PhantomData<S>,
}

impl<S> BroadcastLayer<S> {
    const fn new() -> Self {
        BroadcastLayer {
            _subscriber: PhantomData,
        }
    }
}

impl<S> Layer<S> for BroadcastLayer<S>
where
    S: Subscriber + 'static + for<'lookup> LookupSpan<'lookup>,
{
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        if should_process_tracing_event() {
            let mut log = LogEvent::from(event);
            // Add span fields if available
            if let Some(parent_span) = ctx.event_span(event) {
                for span in parent_span.scope().from_root() {
                    if let Some(fields) = span.extensions().get::<SpanFields>() {
                        for (k, v) in &fields.0 {
                            log.insert(format!("vector.{}", k).as_str(), v.clone());
                        }
                    }
                }
            }
            // Try buffering the event, and if we're not buffering anymore, try to
            // send it along via the trace sender if it's been established.
            if !try_buffer_event(&log) {
                try_broadcast_event(log);
            }
        }
    }

    fn on_new_span(
        &self,
        attrs: &tracing_core::span::Attributes<'_>,
        id: &tracing_core::span::Id,
        ctx: Context<'_, S>,
    ) {
        let span = ctx.span(id).expect("span must already exist!");
        let mut fields = SpanFields::default();
        attrs.values().record(&mut fields);
        span.extensions_mut().insert(fields);
    }
}

#[derive(Default, Debug)]
struct SpanFields(HashMap<&'static str, Value>);

impl SpanFields {
    fn record(&mut self, field: &tracing_core::Field, value: impl Into<Value>) {
        let name = field.name();
        if name.starts_with("component_") {
            self.0.insert(name, value.into());
        }
    }
}

impl tracing::field::Visit for SpanFields {
    fn record_i64(&mut self, field: &tracing_core::Field, value: i64) {
        self.record(field, value);
    }

    fn record_u64(&mut self, field: &tracing_core::Field, value: u64) {
        self.record(field, value);
    }

    fn record_bool(&mut self, field: &tracing_core::Field, value: bool) {
        self.record(field, value);
    }

    fn record_str(&mut self, field: &tracing_core::Field, value: &str) {
        self.record(field, value);
    }

    fn record_debug(&mut self, field: &tracing_core::Field, value: &dyn std::fmt::Debug) {
        self.record(field, format!("{:?}", value));
    }
}
