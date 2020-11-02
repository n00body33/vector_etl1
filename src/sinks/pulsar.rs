use crate::{
    buffers::Acker,
    config::{log_schema, DataType, GenerateConfig, SinkConfig, SinkContext, SinkDescription},
    event::Event,
    sinks::util::encoding::{EncodingConfig, EncodingConfigWithDefault, EncodingConfiguration},
};
use futures::{
    future::BoxFuture, ready, stream::FuturesUnordered, FutureExt, Sink, Stream, TryFutureExt,
};
use pulsar::{
    proto::CommandSendReceipt, Authentication, Error as PulsarError, Producer, Pulsar,
    TokioExecutor,
};
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};
use std::{
    collections::HashSet,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::sync::Mutex;

#[derive(Debug, Snafu)]
enum BuildError {
    #[snafu(display("creating pulsar producer failed: {}", source))]
    CreatePulsarSink { source: PulsarError },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PulsarSinkConfig {
    // Deprecated name
    #[serde(alias = "address")]
    endpoint: String,
    topic: String,
    encoding: EncodingConfigWithDefault<Encoding>,
    auth: Option<AuthConfig>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AuthConfig {
    name: String,  // "token"
    token: String, // <jwt token>
}

#[derive(Clone, Copy, Debug, Derivative, Deserialize, Serialize, Eq, PartialEq)]
#[derivative(Default)]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
    #[derivative(Default)]
    Text,
    Json,
}

struct PulsarSink {
    encoding: EncodingConfig<Encoding>,
    producer: Arc<Mutex<Producer<TokioExecutor>>>,
    in_flight:
        FuturesUnordered<BoxFuture<'static, (usize, Result<CommandSendReceipt, PulsarError>)>>,
    // ack
    seq_head: usize,
    seq_tail: usize,
    pending_acks: HashSet<usize>,
    acker: Acker,
}

inventory::submit! {
    SinkDescription::new::<PulsarSinkConfig>("pulsar")
}

impl GenerateConfig for PulsarSinkConfig {
    fn generate_config() -> toml::Value {
        toml::Value::try_from(Self {
            endpoint: "pulsar://127.0.0.1:6650".to_string(),
            topic: "topic-1234".to_string(),
            encoding: Encoding::Text.into(),
            auth: None,
        })
        .unwrap()
    }
}

#[async_trait::async_trait]
#[typetag::serde(name = "pulsar")]
impl SinkConfig for PulsarSinkConfig {
    async fn build(
        &self,
        cx: SinkContext,
    ) -> crate::Result<(super::VectorSink, super::Healthcheck)> {
        let producer = self
            .create_pulsar_producer()
            .await
            .context(CreatePulsarSink)?;
        let sink = self.clone().new_sink(producer, cx.acker())?;

        let producer = self
            .create_pulsar_producer()
            .await
            .context(CreatePulsarSink)?;
        let healthcheck = healthcheck(producer).boxed();

        Ok((super::VectorSink::Sink(Box::new(sink)), healthcheck))
    }

    fn input_type(&self) -> DataType {
        DataType::Log
    }

    fn sink_type(&self) -> &'static str {
        "pulsar"
    }
}

impl PulsarSinkConfig {
    async fn create_pulsar_producer(&self) -> Result<Producer<TokioExecutor>, PulsarError> {
        let mut builder = Pulsar::builder(&self.endpoint, TokioExecutor);
        if let Some(auth) = &self.auth {
            builder = builder.with_auth(Authentication {
                name: auth.name.clone(),
                data: auth.token.as_bytes().to_vec(),
            });
        }
        let pulsar = builder.build().await?;
        pulsar.producer().with_topic(&self.topic).build().await
    }

    fn new_sink(
        self,
        producer: Producer<TokioExecutor>,
        acker: Acker,
    ) -> crate::Result<PulsarSink> {
        Ok(PulsarSink {
            encoding: self.encoding.into(),
            producer: Arc::new(Mutex::new(producer)),
            in_flight: FuturesUnordered::new(),
            seq_head: 0,
            seq_tail: 0,
            pending_acks: HashSet::new(),
            acker,
        })
    }
}

async fn healthcheck(producer: Producer<TokioExecutor>) -> crate::Result<()> {
    producer.check_connection().await.map_err(Into::into)
}

impl Sink<Event> for PulsarSink {
    type Error = ();

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(mut self: Pin<&mut Self>, item: Event) -> Result<(), Self::Error> {
        let message = encode_event(item, &self.encoding).map_err(|_| ())?;

        let seqno = self.seq_head;
        self.seq_head += 1;

        let producer = Arc::clone(&self.producer);
        self.in_flight.push(Box::pin(async move {
            let mut locked = producer.lock().await;
            (seqno, locked.send(message).and_then(|fut| fut).await)
        }));

        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = Pin::into_inner(self);
        while !this.in_flight.is_empty() {
            match ready!(Pin::new(&mut this.in_flight).poll_next(cx)) {
                Some((seqno, Ok(result))) => {
                    trace!(
                        message = "Pulsar sink produced message.",
                        message_id = ?result.message_id,
                        producer_id = %result.producer_id,
                        sequence_id = %result.sequence_id,
                    );

                    this.pending_acks.insert(seqno);

                    let mut num_to_ack = 0;
                    while this.pending_acks.remove(&this.seq_tail) {
                        num_to_ack += 1;
                        this.seq_tail += 1;
                    }
                    this.acker.ack(num_to_ack);
                }
                Some((_, Err(error))) => {
                    error!(message = "Pulsar sink generated an error.", %error);
                    return Poll::Ready(Err(()));
                }
                None => break,
            }
        }

        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.poll_flush(cx)
    }
}

fn encode_event(mut item: Event, encoding: &EncodingConfig<Encoding>) -> crate::Result<Vec<u8>> {
    encoding.apply_rules(&mut item);
    let log = item.into_log();

    Ok(match encoding.codec() {
        Encoding::Json => serde_json::to_vec(&log)?,
        Encoding::Text => log
            .get(log_schema().message_key())
            .map(|v| v.as_bytes().to_vec())
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn generate_config() {
        crate::test_util::test_generate_config::<PulsarSinkConfig>();
    }

    #[test]
    fn pulsar_event_json() {
        let msg = "hello_world".to_owned();
        let mut evt = Event::from(msg.clone());
        evt.as_mut_log().insert("key", "value");
        let result = encode_event(evt, &EncodingConfig::from(Encoding::Json)).unwrap();
        let map: HashMap<String, String> = serde_json::from_slice(&result[..]).unwrap();
        assert_eq!(msg, map[&log_schema().message_key().to_string()]);
    }

    #[test]
    fn pulsar_event_text() {
        let msg = "hello_world".to_owned();
        let evt = Event::from(msg.clone());
        let event = encode_event(evt, &EncodingConfig::from(Encoding::Text)).unwrap();

        assert_eq!(&event[..], msg.as_bytes());
    }

    #[test]
    fn pulsar_encode_event() {
        let msg = "hello_world";

        let mut evt = Event::from(msg);
        evt.as_mut_log().insert("key", "value");

        let event = encode_event(
            evt,
            &EncodingConfigWithDefault {
                codec: Encoding::Json,
                except_fields: Some(vec!["key".into()]),
                ..Default::default()
            }
            .into(),
        )
        .unwrap();

        let map: HashMap<String, String> = serde_json::from_slice(&event[..]).unwrap();
        assert!(!map.contains_key("key"));
    }
}

#[cfg(feature = "pulsar-integration-tests")]
#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::test_util::{random_lines_with_stream, random_string, trace_init};
    use futures::StreamExt;
    use pulsar::SubType;

    #[tokio::test]
    async fn pulsar_happy() {
        trace_init();

        let num_events = 1_000;
        let (_input, events) = random_lines_with_stream(100, num_events);

        let topic = format!("test-{}", random_string(10));
        let cnf = PulsarSinkConfig {
            endpoint: "pulsar://127.0.0.1:6650".to_owned(),
            topic: topic.clone(),
            encoding: Encoding::Text.into(),
            auth: None,
        };

        let pulsar = Pulsar::<TokioExecutor>::builder(&cnf.endpoint, TokioExecutor)
            .build()
            .await
            .unwrap();
        let mut consumer = pulsar
            .consumer()
            .with_topic(&topic)
            .with_consumer_name("VectorTestConsumer")
            .with_subscription_type(SubType::Shared)
            .with_subscription("VectorTestSub")
            .build::<String>()
            .await
            .unwrap();

        let (acker, ack_counter) = Acker::new_for_testing();
        let producer = cnf.create_pulsar_producer().await.unwrap();
        let sink = cnf.new_sink(producer, acker).unwrap();
        events.map(Ok).forward(sink).await.unwrap();

        assert_eq!(
            ack_counter.load(std::sync::atomic::Ordering::Relaxed),
            num_events
        );

        for _ in 0..num_events {
            let msg = match consumer.next().await.unwrap() {
                Ok(msg) => msg,
                Err(error) => panic!("{:?}", error),
            };
            consumer.ack(&msg).await.unwrap();
        }
    }
}
