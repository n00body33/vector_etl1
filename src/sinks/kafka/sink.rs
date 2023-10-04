use rdkafka::{
    consumer::{BaseConsumer, Consumer},
    error::KafkaError,
    producer::FutureProducer,
    ClientConfig,
};
use snafu::{ResultExt, Snafu};
use tokio::time::Duration;
use tower::limit::ConcurrencyLimit;
use vrl::path::OwnedTargetPath;

use super::config::{KafkaRole, KafkaSinkConfig};
use crate::{
    kafka::{
        KafkaStatisticsContext, KAFKA_DEFAULT_QUEUE_BYTES_MAX, KAFKA_DEFAULT_QUEUE_MESSAGES_MAX,
    },
    sinks::kafka::{request_builder::KafkaRequestBuilder, service::KafkaService},
    sinks::prelude::*,
};

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub(super) enum BuildError {
    #[snafu(display("creating kafka producer failed: {}", source))]
    KafkaCreateFailed { source: KafkaError },
    #[snafu(display("invalid topic template: {}", source))]
    TopicTemplate { source: TemplateParseError },
}

pub struct KafkaSink {
    transformer: Transformer,
    encoder: Encoder<()>,
    service: KafkaService,
    topic: Template,
    key_field: Option<OwnedTargetPath>,
    headers_key: Option<OwnedTargetPath>,
}

pub(crate) fn create_producer(
    client_config: ClientConfig,
) -> crate::Result<FutureProducer<KafkaStatisticsContext>> {
    let producer = client_config
        .create_with_context(KafkaStatisticsContext::default())
        .context(KafkaCreateFailedSnafu)?;
    Ok(producer)
}

impl KafkaSink {
    pub(crate) fn new(config: KafkaSinkConfig) -> crate::Result<Self> {
        let queue_messages_max = config
            .librdkafka_options
            .get("queue.buffering.max.messages")
            .map_or(KAFKA_DEFAULT_QUEUE_MESSAGES_MAX, |v| v.as_str())
            .parse()?;
        let queue_bytes_max = config
            .librdkafka_options
            .get("queue.buffering.max.bytes")
            .map_or(KAFKA_DEFAULT_QUEUE_BYTES_MAX, |v| v.as_str())
            .parse()?;

        let producer_config = config.to_rdkafka(KafkaRole::Producer)?;
        let producer = create_producer(producer_config)?;
        let transformer = config.encoding.transformer();
        let serializer = config.encoding.build()?;
        let encoder = Encoder::<()>::new(serializer);

        Ok(KafkaSink {
            headers_key: config.headers_key.map(|key| key.0),
            transformer,
            encoder,
            service: KafkaService::new(producer, queue_messages_max, queue_bytes_max),
            topic: config.topic,
            key_field: config.key_field.map(|key| key.0),
        })
    }

    async fn run_inner(self: Box<Self>, input: BoxStream<'_, Event>) -> Result<(), ()> {
        let request_builder = KafkaRequestBuilder {
            key_field: self.key_field,
            headers_key: self.headers_key,
            encoder: (self.transformer, self.encoder),
        };

        input
            .filter_map(|event| {
                // Compute the topic.
                future::ready(
                    self.topic
                        .render_string(&event)
                        .map_err(|error| {
                            emit!(TemplateRenderingError {
                                field: None,
                                drop_event: true,
                                error,
                            });
                        })
                        .ok()
                        .map(|topic| (topic, event)),
                )
            })
            .request_builder(default_request_builder_concurrency_limit(), request_builder)
            .filter_map(|request| async {
                match request {
                    Err(error) => {
                        emit!(SinkRequestBuildError { error });
                        None
                    }
                    Ok(req) => Some(req),
                }
            })
            .into_driver(service)
            .protocol("kafka")
            .protocol("kafka")
            .run()
            .await
    }
}

pub(crate) async fn healthcheck(config: KafkaSinkConfig) -> crate::Result<()> {
    trace!("Healthcheck started.");
    let client = config.to_rdkafka(KafkaRole::Consumer).unwrap();
    let topic = match config.topic.render_string(&LogEvent::from_str_legacy("")) {
        Ok(topic) => Some(topic),
        Err(error) => {
            warn!(
                message = "Could not generate topic for healthcheck.",
                %error,
            );
            None
        }
    };

    tokio::task::spawn_blocking(move || {
        let consumer: BaseConsumer = client.create().unwrap();
        let topic = topic.as_ref().map(|topic| &topic[..]);

        consumer
            .fetch_metadata(topic, Duration::from_secs(3))
            .map(|_| ())
    })
    .await??;
    trace!("Healthcheck completed.");
    Ok(())
}

#[async_trait]
impl StreamSink<Event> for KafkaSink {
    async fn run(self: Box<Self>, input: BoxStream<'_, Event>) -> Result<(), ()> {
        self.run_inner(input).await
    }
}
