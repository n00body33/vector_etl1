use crate::{
    codecs::Transformer, event::Event, internal_events::TemplateRenderingError,
    sinks::util::builder::SinkBuilderExt, template::Template,
};
use async_trait::async_trait;
use futures::StreamExt;
use futures_util::stream::BoxStream;
use lapin::options::ConfirmSelectOptions;
use snafu::ResultExt;
use std::{convert::TryFrom, sync::Arc};
use tower::ServiceBuilder;
use vector_core::sink::StreamSink;

use super::{
    config::AMQPSinkConfig, encoder::AMQPEncoder, request_builder::AMQPRequestBuilder,
    service::AMQPService, BuildError, ExchangeTemplateSnafu, RoutingKeyTemplateSnafu,
};

/// Stores the event together with the rendered exchange and routing_key values.
/// This is passed into the `RequestBuilder` which then splits it out into the event
/// and metadata containing the exchange and routing_key.
/// This event needs to be created prior to building the request so we can filter out
/// any events that error whilst redndering the templates.
pub(super) struct AMQPEvent {
    pub(super) event: Event,
    pub(super) exchange: String,
    pub(super) routing_key: String,
}

pub(super) struct AMQPSink {
    pub(super) channel: Arc<lapin::Channel>,
    exchange: Template,
    routing_key: Option<Template>,
    transformer: Transformer,
    encoder: crate::codecs::Encoder<()>,
}

impl AMQPSink {
    pub(super) async fn new(config: AMQPSinkConfig) -> crate::Result<Self> {
        let (_, channel) = config
            .connection
            .connect()
            .await
            .map_err(|e| BuildError::AMQPCreateFailed { source: e })?;

        channel
            .confirm_select(ConfirmSelectOptions::default())
            .await
            .map_err(|e| BuildError::AMQPCreateFailed {
                source: Box::new(e),
            })?;

        let transformer = config.encoding.transformer();
        let serializer = config.encoding.build()?;
        let encoder = crate::codecs::Encoder::<()>::new(serializer);

        Ok(AMQPSink {
            channel: Arc::new(channel),
            exchange: Template::try_from(config.exchange).context(ExchangeTemplateSnafu)?,
            routing_key: config
                .routing_key
                .map(|k| Template::try_from(k).context(RoutingKeyTemplateSnafu))
                .transpose()?,
            transformer,
            encoder,
        })
    }

    /// Transforms an event into an AMQP event by rendering the required template fields.
    /// Returns None if there is an error whilst rendering.
    fn make_amqp_event(&self, event: Event) -> Option<AMQPEvent> {
        let exchange = self
            .exchange
            .render_string(&event)
            .map_err(|missing_keys| {
                emit!(TemplateRenderingError {
                    error: missing_keys,
                    field: Some("exchange"),
                    drop_event: true,
                })
            })
            .ok()?;

        let routing_key = match &self.routing_key {
            None => String::new(),
            Some(key) => key
                .render_string(&event)
                .map_err(|missing_keys| {
                    emit!(TemplateRenderingError {
                        error: missing_keys,
                        field: Some("routing_key"),
                        drop_event: true,
                    })
                })
                .ok()?,
        };

        Some(AMQPEvent {
            event,
            exchange,
            routing_key,
        })
    }

    async fn run_inner(self: Box<Self>, input: BoxStream<'_, Event>) -> Result<(), ()> {
        let request_builder = AMQPRequestBuilder {
            encoder: AMQPEncoder {
                encoder: self.encoder.clone(),
                transformer: self.transformer.clone(),
            },
        };
        let service = ServiceBuilder::new().service(AMQPService {
            channel: Arc::clone(&self.channel),
        });

        let sink = input
            .filter_map(|event| std::future::ready(self.make_amqp_event(event)))
            .request_builder(None, request_builder)
            .filter_map(|request| async move {
                match request {
                    Err(e) => {
                        error!("Failed to build AMQP request: {:?}.", e);
                        None
                    }
                    Ok(req) => Some(req),
                }
            })
            .into_driver(service);

        sink.run().await
    }
}

#[async_trait]
impl StreamSink<Event> for AMQPSink {
    async fn run(self: Box<Self>, input: BoxStream<'_, Event>) -> Result<(), ()> {
        self.run_inner(input).await
    }
}
