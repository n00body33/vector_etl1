//! opendal_common provide real sink supports for all opendal based services.
//!
//! # TODO
//!
//! opendal service now only support very basic sink features. To make it
//! useful, we need to add the following features:
//!
//! - Batch events
//! - Error handling
//! - Limitation
//! - Compression
//! - KeyPartition

use crate::codecs::Encoder;
use crate::codecs::Transformer;
use crate::event::EventFinalizers;
use crate::sinks::util::metadata::RequestMetadataBuilder;
use crate::sinks::util::partitioner::KeyPartitioner;
use crate::sinks::util::{request_builder::EncodeResult, Compression};
use crate::sinks::BoxFuture;
use crate::{
    event::Event,
    internal_events::SinkRequestBuildError,
    sinks::util::{RequestBuilder, SinkBuilderExt},
};
use bytes::Bytes;
use codecs::encoding::Framer;
use futures::{stream::BoxStream, StreamExt};
use opendal::Operator;
use snafu::Snafu;
use std::num::NonZeroUsize;
use std::task::Poll;
use tower::Service;
use vector_common::finalization::{EventStatus, Finalizable};
use vector_common::request_metadata::MetaDescriptive;
use vector_common::request_metadata::RequestMetadata;
use vector_core::internal_event::CountByteSize;
use vector_core::sink::StreamSink;
use vector_core::stream::BatcherSettings;
use vector_core::stream::DriverResponse;
use vector_core::ByteSizeOf;

pub struct OpendalSink {
    op: Operator,
    request_builder: OpendalRequestBuilder,
    partitioner: KeyPartitioner,
    batcher_settings: BatcherSettings,
}

impl OpendalSink {
    pub fn new(op: Operator) -> Self {
        todo!()
    }
}

#[async_trait::async_trait]
impl StreamSink<Event> for OpendalSink {
    async fn run(
        self: Box<Self>,
        input: futures_util::stream::BoxStream<'_, Event>,
    ) -> Result<(), ()> {
        self.run_inner(input).await
    }
}

impl OpendalSink {
    async fn run_inner(self: Box<Self>, input: BoxStream<'_, Event>) -> Result<(), ()> {
        let partitioner = self.partitioner;
        let settings = self.batcher_settings;

        let builder_limit = NonZeroUsize::new(64);
        let request_builder = self.request_builder;

        input
            .batched_partitioned(partitioner, settings)
            .filter_map(|(key, batch)| async move {
                // We don't need to emit an error here if the event is dropped since this will occur if the template
                // couldn't be rendered during the partitioning. A `TemplateRenderingError` is already emitted when
                // that occurs.
                key.map(move |k| (k, batch))
            })
            .request_builder(builder_limit, request_builder)
            .filter_map(|request| async move {
                match request {
                    Err(error) => {
                        emit!(SinkRequestBuildError { error });
                        None
                    }
                    Ok(req) => Some(req),
                }
            })
            .into_driver(OpendalService::new(self.op.clone()))
            .run()
            .await
    }
}

#[derive(Debug, Clone)]
pub struct OpendalService {
    op: Operator,
}

impl OpendalService {
    pub const fn new(op: Operator) -> OpendalService {
        OpendalService { op }
    }
}

pub struct OpendalRequest {
    pub payload: Bytes,
    pub metadata: OpendalMetadata,
    pub request_metadata: RequestMetadata,
}

impl MetaDescriptive for OpendalRequest {
    fn get_metadata(&self) -> RequestMetadata {
        self.request_metadata
    }
}

impl Finalizable for OpendalRequest {
    fn take_finalizers(&mut self) -> EventFinalizers {
        std::mem::take(&mut self.metadata.finalizers)
    }
}

pub struct OpendalMetadata {
    pub partition_key: String,
    pub count: usize,
    pub byte_size: usize,
    pub finalizers: EventFinalizers,
}

struct OpendalRequestBuilder {
    pub encoder: (Transformer, Encoder<Framer>),
    pub compression: Compression,
}

impl RequestBuilder<(String, Vec<Event>)> for OpendalRequestBuilder {
    type Metadata = OpendalMetadata;
    type Events = Vec<Event>;
    type Encoder = (Transformer, Encoder<Framer>);
    type Payload = Bytes;
    type Request = OpendalRequest;
    type Error = std::io::Error;

    fn compression(&self) -> Compression {
        self.compression
    }

    fn encoder(&self) -> &Self::Encoder {
        &self.encoder
    }

    fn split_input(
        &self,
        input: (String, Vec<Event>),
    ) -> (Self::Metadata, RequestMetadataBuilder, Self::Events) {
        let (partition_key, mut events) = input;
        let finalizers = events.take_finalizers();
        let opendal_metadata = OpendalMetadata {
            partition_key,
            count: events.len(),
            byte_size: events.size_of(),
            finalizers,
        };

        let builder = RequestMetadataBuilder::from_events(&events);

        (opendal_metadata, builder, events)
    }

    fn build_request(
        &self,
        mut metadata: Self::Metadata,
        request_metadata: RequestMetadata,
        payload: EncodeResult<Self::Payload>,
    ) -> Self::Request {
        // TODO: we can support time format later.
        let name = uuid::Uuid::new_v4().to_string();
        let extension = self.compression.extension();

        metadata.partition_key = format!("{}{}.{}", metadata.partition_key, name, extension);

        OpendalRequest {
            metadata,
            payload: payload.into_payload(),
            request_metadata: request_metadata,
        }
    }
}

#[derive(Debug)]
pub struct OpendalResponse {
    byte_size: usize,
}

impl DriverResponse for OpendalResponse {
    fn event_status(&self) -> EventStatus {
        EventStatus::Delivered
    }

    fn events_sent(&self) -> CountByteSize {
        // (events count, byte size)
        CountByteSize(1, self.byte_size)
    }
}

impl Service<OpendalRequest> for OpendalService {
    type Response = OpendalResponse;
    type Error = opendal::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: OpendalRequest) -> Self::Future {
        let byte_size = request.payload.len();
        let op = self.op.clone();

        Box::pin(async move {
            let result = op
                .object(&request.metadata.partition_key.as_str())
                .write(request.payload)
                .await;
            result.map(|_| OpendalResponse { byte_size })
        })
    }
}

#[derive(Debug, Snafu)]
pub enum OpendalError {
    #[snafu(display("Failed to call OpenDAL: {}", source))]
    OpenDAL { source: opendal::Error },
}

impl From<opendal::Error> for OpendalError {
    fn from(source: opendal::Error) -> Self {
        Self::OpenDAL { source }
    }
}
