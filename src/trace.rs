use crate::event::{Event, LogEvent, Value};
use metrics_tracing_context::MetricsLayer;
use once_cell::sync::OnceCell;
use std::{
    convert::TryInto,
    fmt::Debug,
    sync::{Mutex, MutexGuard},
};
use tokio::sync::broadcast::{self, Receiver, Sender};
use tracing::{
    dispatcher::{set_global_default, Dispatch},
    field::{Field, Visit},
    span::Span,
    subscriber::Interest,
    Id, Metadata, Subscriber,
};
use tracing_core::span;
use tracing_limit::Limit;
use tracing_log::LogTracer;
use tracing_subscriber::{layer::SubscriberExt, FmtSubscriber};

/// BUFFER contains all of the internal log events generated by Vector
/// before the topology has been initialized. It will be cleared (set to
/// `None`) by the topology initialization routines.
static BUFFER: OnceCell<Mutex<Option<Vec<Event>>>> = OnceCell::new();

/// SENDER holds the sender/receiver handle that will receive a copy of
/// all the internal log events *after* the topology has been
/// initialized.
static SENDER: OnceCell<Sender<Event>> = OnceCell::new();

pub use tracing_futures::Instrument;
pub use tracing_tower::{InstrumentableService, InstrumentedService};

pub fn init(color: bool, json: bool, levels: &str) {
    let _ = BUFFER.set(Mutex::new(Some(Vec::new())));

    let dispatch = if json {
        let formatter = FmtSubscriber::builder()
            .with_env_filter(levels)
            .json()
            .flatten_event(true)
            .finish()
            .with(Limit::default())
            .with(MetricsLayer::new());

        Dispatch::new(BroadcastSubscriber { formatter })
    } else {
        let formatter = FmtSubscriber::builder()
            .with_ansi(color)
            .with_env_filter(levels)
            .finish()
            .with(Limit::default())
            .with(MetricsLayer::new());
        Dispatch::new(BroadcastSubscriber { formatter })
    };

    let _ = LogTracer::init();
    let _ = set_global_default(dispatch);
}

fn early_buffer() -> MutexGuard<'static, Option<Vec<Event>>> {
    BUFFER
        .get()
        .expect("Internal logs buffer not initialized.")
        .lock()
        .expect("Couldn't acquire lock on internal logs buffer.")
}

pub fn stop_buffering() {
    *early_buffer() = None;
}

pub fn current_span() -> Span {
    Span::current()
}

pub struct TraceSubscription {
    pub buffer: Vec<Event>,
    pub receiver: Receiver<Event>,
}

pub fn subscribe() -> TraceSubscription {
    let buffer = match early_buffer().as_mut() {
        Some(buffer) => buffer.drain(..).collect(),
        None => Vec::new(),
    };
    let receiver = SENDER.get_or_init(|| broadcast::channel(99).0).subscribe();
    TraceSubscription { buffer, receiver }
}

struct BroadcastSubscriber<F> {
    formatter: F,
}

impl<F: Subscriber + 'static> Subscriber for BroadcastSubscriber<F> {
    #[inline]
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.formatter.enabled(metadata)
    }

    #[inline]
    fn new_span(&self, span: &tracing::span::Attributes<'_>) -> Id {
        self.formatter.new_span(span)
    }

    #[inline]
    fn record(&self, span: &Id, record: &tracing::span::Record<'_>) {
        self.formatter.record(span, record)
    }

    #[inline]
    fn record_follows_from(&self, span: &Id, follows: &Id) {
        self.formatter.record_follows_from(span, follows)
    }

    #[inline]
    fn event(&self, event: &tracing::Event<'_>) {
        if let Some(buffer) = early_buffer().as_mut() {
            buffer.push(event.into());
        }
        if let Some(sender) = SENDER.get() {
            let _ = sender.send(event.into()); // Ignore errors
        }
        self.formatter.event(event)
    }

    #[inline]
    fn enter(&self, span: &Id) {
        self.formatter.enter(span)
    }

    #[inline]
    fn exit(&self, span: &Id) {
        self.formatter.exit(span)
    }

    #[inline]
    fn current_span(&self) -> span::Current {
        self.formatter.current_span()
    }

    #[inline]
    fn clone_span(&self, id: &Id) -> Id {
        self.formatter.clone_span(id)
    }

    #[inline]
    fn try_close(&self, id: Id) -> bool {
        self.formatter.try_close(id)
    }

    #[inline]
    fn register_callsite(&self, meta: &'static Metadata<'static>) -> Interest {
        self.formatter.register_callsite(meta)
    }

    #[inline]
    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        self.formatter.max_level_hint()
    }

    #[inline]
    unsafe fn downcast_raw(&self, id: std::any::TypeId) -> Option<*const ()> {
        self.formatter.downcast_raw(id)
    }
}

impl From<&tracing::Event<'_>> for Event {
    fn from(event: &tracing::Event<'_>) -> Self {
        let now = chrono::Utc::now();
        let mut maker = MakeLogEvent::default();
        event.record(&mut maker);

        let mut log = maker.0;
        log.insert("timestamp", now);

        let meta = event.metadata();
        log.insert(
            "metadata.kind",
            if meta.is_event() {
                Value::Bytes("event".to_string().into())
            } else if meta.is_span() {
                Value::Bytes("span".to_string().into())
            } else {
                Value::Null
            },
        );
        log.insert("metadata.level", meta.level().to_string());
        log.insert(
            "metadata.module_path",
            meta.module_path()
                .map(|mp| Value::Bytes(mp.to_string().into()))
                .unwrap_or(Value::Null),
        );
        log.insert("metadata.target", meta.target().to_string());

        log.into()
    }
}

#[derive(Debug, Default)]
struct MakeLogEvent(LogEvent);

impl Visit for MakeLogEvent {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name(), value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.0.insert(field.name(), format!("{:?}", value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.0.insert(field.name(), value);
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        let field = field.name();
        let converted: Result<i64, _> = value.try_into();
        match converted {
            Ok(value) => self.0.insert(field, value),
            Err(_) => self.0.insert(field, value.to_string()),
        };
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.0.insert(field.name(), value);
    }
}
