use bytes::Bytes;
use chrono::Utc;
use codecs::encoding::Framer;
use uuid::Uuid;
use vector_core::ByteSizeOf;

use crate::{
    codecs::{Encoder, Transformer},
    event::{Event, Finalizable},
    sinks::{
        azure_common::config::{AzureBlobMetadata, AzureBlobRequest},
        util::{
            metadata::RequestMetadataBuilder, request_builder::EncodeResult, Compression,
            RequestBuilder,
        },
    },
};

#[derive(Clone)]
pub struct AzureBlobRequestOptions {
    pub container_name: String,
    pub blob_time_format: String,
    pub blob_append_uuid: bool,
    pub encoder: (Transformer, Encoder<Framer>),
    pub compression: Compression,
}

impl RequestBuilder<(String, Vec<Event>)> for AzureBlobRequestOptions {
    type Metadata = (AzureBlobMetadata, RequestMetadataBuilder);
    type Events = Vec<Event>;
    type Encoder = (Transformer, Encoder<Framer>);
    type Payload = Bytes;
    type Request = AzureBlobRequest;
    type Error = std::io::Error;

    fn compression(&self) -> Compression {
        self.compression
    }

    fn encoder(&self) -> &Self::Encoder {
        &self.encoder
    }

    fn split_input(&self, input: (String, Vec<Event>)) -> (Self::Metadata, Self::Events) {
        let (partition_key, mut events) = input;
        let finalizers = events.take_finalizers();
        let metadata = AzureBlobMetadata {
            partition_key,
            count: events.len(),
            byte_size: events.size_of(),
            finalizers,
        };

        let builder = RequestMetadataBuilder::from_events(&events);

        ((metadata, builder), events)
    }

    fn build_request(
        &self,
        metadata: Self::Metadata,
        payload: EncodeResult<Self::Payload>,
    ) -> Self::Request {
        let (mut azure_metadata, builder) = metadata;

        let blob_name = {
            let formatted_ts = Utc::now().format(self.blob_time_format.as_str());

            self.blob_append_uuid
                .then(|| format!("{}-{}", formatted_ts, Uuid::new_v4().hyphenated()))
                .unwrap_or_else(|| formatted_ts.to_string())
        };

        let extension = self.compression.extension();
        azure_metadata.partition_key = format!(
            "{}{}.{}",
            azure_metadata.partition_key, blob_name, extension
        );

        let request_metadata = builder.build(&payload);
        let payload_bytes = payload.into_payload();

        debug!(
            message = "Sending events.",
            bytes = ?payload_bytes.len(),
            events_len = ?azure_metadata.count,
            blob = ?azure_metadata.partition_key,
            container = ?self.container_name,
        );

        AzureBlobRequest {
            blob_data: payload_bytes,
            content_encoding: self.compression.content_encoding(),
            content_type: self.compression.content_type(),
            metadata: azure_metadata,
            request_metadata,
        }
    }
}

impl Compression {
    pub const fn content_type(self) -> &'static str {
        match self {
            Self::None => "text/plain",
            Self::Gzip(_) => "application/gzip",
            Self::Zlib(_) => "application/zlib",
        }
    }
}
