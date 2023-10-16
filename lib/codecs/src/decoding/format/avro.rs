use super::Deserializer;
use crate::encoding::AvroSerializerOptions;
use bytes::Buf;
use bytes::Bytes;
use chrono::{TimeZone, Utc};
use lookup::event_path;
use serde::{Deserialize, Serialize};
use smallvec::{smallvec, SmallVec};
use vector_config::configurable_component;
use vector_core::{
    config::{log_schema, DataType, LogNamespace},
    event::{Event, LogEvent},
    schema,
};

type VrlValue = vrl::value::Value;
type AvroValue = apache_avro::types::Value;

const CONFLUENT_MAGIC_BYTE: u8 = 0;
const CONFLUENT_SCHEMA_PREFIX_LEN: usize = 5;

/// Config used to build a `AvroDeserializer`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AvroDeserializerConfig {
    /// Options for the Avro deserializer.
    pub avro_options: AvroDeserializerOptions,
}

impl AvroDeserializerConfig {
    /// Creates a new `AvroDeserializerConfig`.
    pub const fn new(schema: String, strip_schema_id_prefix: bool) -> Self {
        Self {
            avro_options: AvroDeserializerOptions {
                schema,
                strip_schema_id_prefix,
            },
        }
    }

    /// Build the `AvroDeserializer` from this configuration.
    pub fn build(&self) -> AvroDeserializer {
        let schema = apache_avro::Schema::parse_str(&self.avro_options.schema)
            .map_err(|error| format!("Failed building Avro serializer: {}", error))
            .unwrap();
        AvroDeserializer {
            schema,
            strip_schema_id_prefix: self.avro_options.strip_schema_id_prefix,
        }
    }

    /// The data type of events that are accepted by `AvroDeserializer`.
    pub fn output_type(&self) -> DataType {
        DataType::Log
    }

    /// The schema required by the serializer.
    pub fn schema_definition(&self, log_namespace: LogNamespace) -> schema::Definition {
        match log_namespace {
            LogNamespace::Legacy => {
                let mut definition = schema::Definition::empty_legacy_namespace()
                    .unknown_fields(vrl::value::Kind::any());

                if let Some(timestamp_key) = log_schema().timestamp_key() {
                    definition = definition.try_with_field(
                        timestamp_key,
                        vrl::value::Kind::any().or_timestamp(),
                        Some("timestamp"),
                    );
                }
                definition
            }
            LogNamespace::Vector => schema::Definition::new_with_default_metadata(
                vrl::value::Kind::any(),
                [log_namespace],
            ),
        }
    }
}

impl From<&AvroDeserializerOptions> for AvroSerializerOptions {
    fn from(value: &AvroDeserializerOptions) -> Self {
        Self {
            schema: value.schema.clone(),
        }
    }
}
/// Apache Avro serializer options.
#[configurable_component]
#[derive(Clone, Debug)]
pub struct AvroDeserializerOptions {
    /// The Avro schema.
    #[configurable(metadata(
        docs::examples = r#"{ "type": "record", "name": "log", "fields": [{ "name": "message", "type": "string" }] }"#
    ))]
    pub schema: String,

    /// For avro datum encoded in kafka messages, the bytes are prefixed with the schema id.  Set this to true to strip the schema id prefix.
    /// According to [Confluent Kafka's document](https://docs.confluent.io/platform/current/schema-registry/fundamentals/serdes-develop/index.html#wire-format).
    pub strip_schema_id_prefix: bool,
}

/// Serializer that converts bytes to an `Event` using the Apache Avro format.
#[derive(Debug, Clone)]
pub struct AvroDeserializer {
    schema: apache_avro::Schema,
    strip_schema_id_prefix: bool,
}

impl AvroDeserializer {
    /// Creates a new `AvroDeserializer`.
    pub const fn new(schema: apache_avro::Schema, strip_schema_id_prefix: bool) -> Self {
        Self {
            schema,
            strip_schema_id_prefix,
        }
    }
}

impl Deserializer for AvroDeserializer {
    fn parse(
        &self,
        bytes: Bytes,
        log_namespace: LogNamespace,
    ) -> vector_common::Result<SmallVec<[Event; 1]>> {
        // Avro has a `null` type which indicates no value.
        if bytes.is_empty() {
            return Ok(smallvec![]);
        }

        let bytes = if self.strip_schema_id_prefix {
            if bytes.len() >= CONFLUENT_SCHEMA_PREFIX_LEN && bytes[0] == CONFLUENT_MAGIC_BYTE {
                bytes.slice(CONFLUENT_SCHEMA_PREFIX_LEN..)
            } else {
                return Err(vector_common::Error::from(
                    "Expected avro datum to be prefixed with schema id",
                ));
            }
        } else {
            bytes
        };

        let value = apache_avro::from_avro_datum(&self.schema, &mut bytes.reader(), None)?;

        let apache_avro::types::Value::Record(fields) = value else {
            return Err(vector_common::Error::from("Expected an avro Record"));
        };

        let mut log = LogEvent::default();
        for (k, v) in fields {
            log.insert(event_path!(k.as_str()), try_from(v)?);
        }

        let mut event = Event::Log(log);
        let event = match log_namespace {
            LogNamespace::Vector => event,
            LogNamespace::Legacy => {
                if let Some(timestamp_key) = log_schema().timestamp_key_target_path() {
                    let log = event.as_mut_log();
                    if !log.contains(timestamp_key) {
                        let timestamp = Utc::now();
                        log.insert(timestamp_key, timestamp);
                    }
                }
                event
            }
        };
        Ok(smallvec![event])
    }
}

// Can't use std::convert::TryFrom because of orphan rules
pub fn try_from(value: AvroValue) -> vector_common::Result<VrlValue> {
    // Very similar to avro to json see `impl std::convert::TryFrom<AvroValue> for serde_json::Value`
    // LogEvent has native support for bytes, so it is used for Bytes and Fixed
    match value {
        AvroValue::Array(array) => {
            let mut vector = Vec::new();
            for item in array {
                vector.push(try_from(item)?);
            }
            Ok(VrlValue::Array(vector))
        }
        AvroValue::Boolean(boolean) => Ok(VrlValue::from(boolean)),
        AvroValue::Bytes(bytes) => Ok(VrlValue::from(bytes)),
        AvroValue::Date(d) => Ok(VrlValue::from(d)),
        AvroValue::Decimal(ref d) => Ok(<Vec<u8>>::try_from(d)
            .map(|vec| VrlValue::Array(vec.into_iter().map(VrlValue::from).collect()))?),
        AvroValue::Double(double) => Ok(VrlValue::from_f64_or_zero(double)),
        AvroValue::Duration(d) => Ok(VrlValue::Array(
            <[u8; 12]>::from(d)
                .into_iter()
                .map(VrlValue::from)
                .collect(),
        )),
        AvroValue::Enum(_, string) => Ok(VrlValue::from(string)),
        AvroValue::Fixed(_, bytes) => Ok(VrlValue::from(bytes)),
        AvroValue::Float(float) => Ok(VrlValue::from_f64_or_zero(float as f64)),
        AvroValue::Int(int) => Ok(VrlValue::from(int)),
        AvroValue::Long(long) => Ok(VrlValue::from(long)),
        AvroValue::Map(items) => items
            .into_iter()
            .map(|(key, value)| try_from(value).map(|v| (key, v)))
            .collect::<Result<Vec<_>, _>>()
            .map(|v| VrlValue::Object(v.into_iter().collect())),
        AvroValue::Null => Ok(VrlValue::Null),
        AvroValue::Record(items) => items
            .into_iter()
            .map(|(key, value)| try_from(value).map(|v| (key, v)))
            .collect::<Result<Vec<_>, _>>()
            .map(|v| VrlValue::Object(v.into_iter().collect())),
        AvroValue::String(string) => Ok(VrlValue::from(string)),
        AvroValue::TimeMicros(time_micros) => Ok(VrlValue::from(time_micros)),
        AvroValue::TimeMillis(time_millis) => Ok(VrlValue::from(time_millis)),
        AvroValue::TimestampMicros(micros) => Ok(VrlValue::Timestamp(
            Utc.timestamp_micros(micros)
                .single()
                .ok_or("failed to convert TimestampMicros")?,
        )),
        AvroValue::TimestampMillis(millis) => Ok(VrlValue::Timestamp(
            Utc.timestamp_millis_opt(millis)
                .single()
                .ok_or("failed to convert TimestampMillis")?,
        )),
        AvroValue::Union(_, v) => try_from(*v),
        AvroValue::Uuid(uuid) => Ok(VrlValue::from(uuid.as_hyphenated().to_string())),
        AvroValue::LocalTimestampMillis(_) | AvroValue::LocalTimestampMicros(_) => unimplemented!(),
    }
}

#[cfg(test)]
mod tests {
    use apache_avro::Schema;
    use bytes::BytesMut;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn deserialize_avro() {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        struct Log {
            message: String,
        }

        let schema = r#"
            {
                "type": "record",
                "name": "log",
                "fields": [
                    {
                        "name": "message",
                        "type": "string"
                    }
                ]
            }
        "#
        .to_owned();
        let schema = Schema::parse_str(&schema).unwrap();

        let event = Log {
            message: "hello from avro".to_owned(),
        };
        let record_value = apache_avro::to_value(event).unwrap();
        let record_datum = apache_avro::to_avro_datum(&schema, record_value).unwrap();
        let record_bytes = Bytes::from(record_datum);

        let deserializer = AvroDeserializer::new(schema, false);
        let events = deserializer
            .parse(record_bytes, LogNamespace::Vector)
            .unwrap();
        assert_eq!(events.len(), 1);

        assert_eq!(
            events[0].as_log().get("message").unwrap(),
            &VrlValue::from("hello from avro")
        );
    }

    #[test]
    fn deserialize_avro_strip_schema_id_prefix() {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        struct Log {
            message: String,
        }

        let schema = r#"
            {
                "type": "record",
                "name": "log",
                "fields": [
                    {
                        "name": "message",
                        "type": "string"
                    }
                ]
            }
        "#
        .to_owned();
        let schema = Schema::parse_str(&schema).unwrap();

        let event = Log {
            message: "hello from avro".to_owned(),
        };
        let record_value = apache_avro::to_value(event).unwrap();
        let record_datum = apache_avro::to_avro_datum(&schema, record_value).unwrap();

        let mut bytes = BytesMut::new();
        bytes.extend([0, 0, 0, 0, 0]); // 0 prefix + 4 byte schema id
        bytes.extend(record_datum);

        let deserializer = AvroDeserializer::new(schema, true);
        let events = deserializer
            .parse(bytes.freeze(), LogNamespace::Vector)
            .unwrap();
        assert_eq!(events.len(), 1);

        assert_eq!(
            events[0].as_log().get("message").unwrap(),
            &VrlValue::from("hello from avro")
        );
    }

    #[test]
    fn deserialize_avro_uuid() {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        struct Log {
            uuid: String,
        }

        let raw_schema = r#"
            {
                "type": "record",
                "name": "log",
                "fields": [
                    {
                        "name": "uuid",
                        "type": "string",
                        "logicalType":"uuid"
                    }
                ]
            }
        "#
        .to_owned();
        let schema = Schema::parse_str(&raw_schema).unwrap();

        let uuid = Uuid::new_v4().hyphenated().to_string();
        let event = Log { uuid: uuid.clone() };
        let value = apache_avro::to_value(event).unwrap();
        // let value = value.resolve(&schema).unwrap();
        let datum = apache_avro::to_avro_datum(&schema, value).unwrap();

        let mut bytes = BytesMut::new();
        bytes.extend([0, 0, 0, 0, 0]); // 0 prefix + 4 byte schema id
        bytes.extend(datum);

        let deserializer = AvroDeserializer::new(schema, true);
        let events = deserializer
            .parse(bytes.freeze(), LogNamespace::Vector)
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].as_log().get("uuid").unwrap(),
            &VrlValue::from(uuid)
        );
    }
}
