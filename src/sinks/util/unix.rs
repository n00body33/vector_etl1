use crate::{
    buffers::Acker,
    config::SinkContext,
    internal_events::{
        UnixSocketConnectionEstablished, UnixSocketConnectionFailure, UnixSocketError,
        UnixSocketEventSent,
    },
    sinks::{
        util::{
            acker_bytes_sink::{AckerBytesSink, ShutdownCheck},
            StreamSink,
        },
        Healthcheck, VectorSink,
    },
    Event,
};
use async_trait::async_trait;
use bytes::Bytes;
use futures::{future::BoxFuture, stream::BoxStream, SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};
use std::{path::PathBuf, pin::Pin, sync::Arc, time::Duration};
use tokio::{net::UnixStream, time::delay_for};
use tokio_retry::strategy::ExponentialBackoff;

#[derive(Debug, Snafu)]
pub enum HealthcheckError {
    #[snafu(display("Connect error: {}", source))]
    ConnectError { source: tokio::io::Error },
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct UnixSinkConfig {
    pub path: PathBuf,
}

impl UnixSinkConfig {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn build(
        &self,
        cx: SinkContext,
        encode_event: impl Fn(Event) -> Option<Bytes> + Send + Sync + 'static,
    ) -> crate::Result<(VectorSink, Healthcheck)> {
        let sink = UnixSink::new(self.path.clone(), cx.acker(), encode_event);
        Ok((VectorSink::Stream(Box::new(sink)), self.healthcheck()))
    }

    fn healthcheck(&self) -> BoxFuture<'static, crate::Result<()>> {
        let path = self.path.clone();

        Box::pin(async move {
            UnixStream::connect(&path)
                .await
                .context(ConnectError)
                .map(|_| ())
                .map_err(Into::into)
        })
    }
}

struct UnixSink {
    path: PathBuf,
    acker: Acker,
    encode_event: Arc<dyn Fn(Event) -> Option<Bytes> + Send + Sync>,
}

impl UnixSink {
    pub fn new(
        path: PathBuf,
        acker: Acker,
        encode_event: impl Fn(Event) -> Option<Bytes> + Send + Sync + 'static,
    ) -> Self {
        Self {
            path,
            acker,
            encode_event: Arc::new(encode_event),
        }
    }

    fn fresh_backoff() -> ExponentialBackoff {
        // TODO: make configurable
        ExponentialBackoff::from_millis(2)
            .factor(250)
            .max_delay(Duration::from_secs(60))
    }

    async fn connect(&mut self) -> AckerBytesSink<UnixStream> {
        let mut backoff = Self::fresh_backoff();
        loop {
            debug!(
                message = "Connecting",
                path = %self.path.to_str().unwrap()
            );
            match UnixStream::connect(self.path.clone()).await {
                Ok(stream) => {
                    emit!(UnixSocketConnectionEstablished { path: &self.path });
                    return AckerBytesSink::new(
                        stream,
                        self.acker.clone(),
                        Box::new(|byte_size| emit!(UnixSocketEventSent { byte_size })),
                        Box::new(|_| ShutdownCheck::Alive),
                    );
                }
                Err(error) => {
                    emit!(UnixSocketConnectionFailure {
                        error,
                        path: &self.path
                    });
                    delay_for(backoff.next().unwrap()).await;
                }
            }
        }
    }
}

#[async_trait]
impl StreamSink for UnixSink {
    async fn run(&mut self, input: BoxStream<'_, Event>) -> Result<(), ()> {
        let encode_event = Arc::clone(&self.encode_event);
        let mut input = input
            // We send event empty events because `AckerBytesSink` `ack` and `emit!` for us.
            .map(|event| match encode_event(event) {
                Some(bytes) => bytes,
                None => Bytes::new(),
            })
            .map(Ok)
            .peekable();

        while Pin::new(&mut input).peek().await.is_some() {
            let mut sink = self.connect().await;
            if let Err(error) = sink.send_all(&mut input).await {
                emit!(UnixSocketError {
                    error,
                    path: &self.path
                });
            }
            // TODO: we can lost ack for buffered item
            // https://docs.rs/futures-util/0.3.6/src/futures_util/sink/send_all.rs.html#78-112
            sink.ack();
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sinks::util::{encode_event, Encoding};
    use crate::test_util::{random_lines_with_stream, CountReceiver};
    use tokio::net::UnixListener;

    fn temp_uds_path(name: &str) -> PathBuf {
        tempfile::tempdir().unwrap().into_path().join(name)
    }

    #[tokio::test]
    async fn unix_sink_healthcheck() {
        let good_path = temp_uds_path("valid_uds");
        let _listener = UnixListener::bind(&good_path).unwrap();
        assert!(UnixSinkConfig::new(good_path).healthcheck().await.is_ok());

        let bad_path = temp_uds_path("no_one_listening");
        assert!(UnixSinkConfig::new(bad_path).healthcheck().await.is_err());
    }

    #[tokio::test]
    async fn basic_unix_sink() {
        let num_lines = 1000;
        let out_path = temp_uds_path("unix_test");

        // Set up server to receive events from the Sink.
        let mut receiver = CountReceiver::receive_lines_unix(out_path.clone());

        // Set up Sink
        let config = UnixSinkConfig::new(out_path);
        let cx = SinkContext::new_test();
        let encoding = Encoding::Text.into();
        let (sink, _healthcheck) = config
            .build(cx, move |event| encode_event(event, &encoding))
            .unwrap();

        // Send the test data
        let (input_lines, events) = random_lines_with_stream(100, num_lines);
        sink.run(events).await.unwrap();

        // Wait for output to connect
        receiver.connected().await;

        // Receive the data sent by the Sink to the receiver
        assert_eq!(input_lines, receiver.await);
    }
}
