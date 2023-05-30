use std::num::NonZeroUsize;

use vector_buffers::EventCount;
use vector_core::{ByteSizeOf, EstimatedJsonEncodedSizeOf};

use vector_common::{
    internal_event::CountByteSize,
    request_metadata::RequestMetadata,
    request_metadata::{GetEventCountTags, RequestCountByteSize},
};

use super::request_builder::EncodeResult;

#[derive(Default, Clone)]
pub struct RequestMetadataBuilder {
    event_count: usize,
    events_byte_size: usize,
    events_estimated_json_encoded_byte_size: RequestCountByteSize,
}

impl RequestMetadataBuilder {
    pub fn from_events<E>(events: E) -> Self
    where
        E: ByteSizeOf + EventCount + EstimatedJsonEncodedSizeOf,
    {
        Self {
            event_count: events.event_count(),
            events_byte_size: events.size_of(),
            events_estimated_json_encoded_byte_size: CountByteSize(
                events.event_count(),
                events.estimated_json_encoded_size_of(),
            )
            .into(),
        }
    }

    pub fn from_event<E>(event: &E) -> Self
    where
        E: ByteSizeOf + GetEventCountTags + EstimatedJsonEncodedSizeOf,
    {
        let mut size = RequestCountByteSize::default();

        size.add_event(event, event.estimated_json_encoded_size_of());

        Self {
            event_count: 1,
            events_byte_size: event.size_of(),
            events_estimated_json_encoded_byte_size: size,
        }
    }

    pub fn new(
        event_count: usize,
        events_byte_size: usize,
        events_estimated_json_encoded_byte_size: usize,
    ) -> Self {
        Self {
            event_count,
            events_byte_size,
            events_estimated_json_encoded_byte_size: CountByteSize(
                event_count,
                events_estimated_json_encoded_byte_size,
            )
            .into(),
        }
    }

    pub fn increment(&mut self, event_count: usize, events_byte_size: usize) {
        self.event_count += event_count;
        self.events_byte_size += events_byte_size;
    }

    pub fn with_request_size(&self, size: NonZeroUsize) -> RequestMetadata {
        let size = size.get();

        RequestMetadata::new(
            self.event_count,
            self.events_byte_size,
            size,
            size,
            self.events_estimated_json_encoded_byte_size.clone(),
        )
    }

    pub fn build<T>(&self, result: &EncodeResult<T>) -> RequestMetadata {
        RequestMetadata::new(
            self.event_count,
            self.events_byte_size,
            result.uncompressed_byte_size,
            result
                .compressed_byte_size
                .unwrap_or(result.uncompressed_byte_size),
            self.events_estimated_json_encoded_byte_size.clone(),
        )
    }
}
