use crate::codecs::Transformer;
use codecs::{
    encoding::{Framer, FramingConfig, Serializer, SerializerConfig},
    CharacterDelimitedEncoder, LengthDelimitedEncoder, NewlineDelimitedEncoder,
};
use vector_config::configurable_component;

/// Encoding configuration.
#[configurable_component]
#[derive(Clone, Debug)]
pub struct EncodingConfig {
    #[serde(flatten)]
    encoding: SerializerConfig,

    #[serde(flatten)]
    transformer: Transformer,
}

impl EncodingConfig {
    /// Creates a new `EncodingConfig` with the provided `SerializerConfig` and `Transformer`.
    pub const fn new(encoding: SerializerConfig, transformer: Transformer) -> Self {
        Self {
            encoding,
            transformer,
        }
    }

    /// Build a `Transformer` that applies the encoding rules to an event before serialization.
    pub fn transformer(&self) -> Transformer {
        self.transformer.clone()
    }

    /// Get the encoding configuration.
    pub const fn config(&self) -> &SerializerConfig {
        &self.encoding
    }

    /// Build the `Serializer` for this config.
    pub fn build(&self) -> crate::Result<Serializer> {
        self.encoding.build()
    }
}

impl<T> From<T> for EncodingConfig
where
    T: Into<SerializerConfig>,
{
    fn from(encoding: T) -> Self {
        Self {
            encoding: encoding.into(),
            transformer: Default::default(),
        }
    }
}

/// Encoding configuration.
#[configurable_component]
#[derive(Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct EncodingConfigWithFraming {
    #[configurable(derived)]
    framing: Option<FramingConfig>,

    #[configurable(derived)]
    encoding: EncodingConfig,
}

impl EncodingConfigWithFraming {
    /// Creates a new `EncodingConfigWithFraming` with the provided `FramingConfig`,
    /// `SerializerConfig` and `Transformer`.
    pub const fn new(
        framing: Option<FramingConfig>,
        encoding: SerializerConfig,
        transformer: Transformer,
    ) -> Self {
        Self {
            framing,
            encoding: EncodingConfig {
                encoding,
                transformer,
            },
        }
    }

    /// Build a `Transformer` that applies the encoding rules to an event before serialization.
    pub fn transformer(&self) -> Transformer {
        self.encoding.transformer.clone()
    }

    /// Get the encoding configuration.
    pub const fn config(&self) -> (&Option<FramingConfig>, &SerializerConfig) {
        (&self.framing, &self.encoding.encoding)
    }

    /// Build the `Framer` and `Serializer` for this config.
    pub fn build(&self, sink_type: SinkType) -> crate::Result<(Framer, Serializer)> {
        let framer = self.framing.as_ref().map(|framing| framing.build());
        let serializer = self.encoding.build()?;

        let framer = match (framer, &serializer) {
            (Some(framer), _) => framer,
            (None, Serializer::Json(_)) => match sink_type {
                SinkType::StreamBased => NewlineDelimitedEncoder::new().into(),
                SinkType::MessageBased => CharacterDelimitedEncoder::new(b',').into(),
            },
            (None, Serializer::Avro(_) | Serializer::Native(_)) => {
                LengthDelimitedEncoder::new().into()
            }
            (
                None,
                Serializer::Gelf(_)
                | Serializer::Logfmt(_)
                | Serializer::NativeJson(_)
                | Serializer::RawMessage(_)
                | Serializer::Text(_),
            ) => NewlineDelimitedEncoder::new().into(),
        };

        Ok((framer, serializer))
    }
}

/// The way a sink processes outgoing events.
pub enum SinkType {
    /// Events are sent in a continuous stream.
    StreamBased,
    /// Events are sent in a batch as a message.
    MessageBased,
}

impl<F, S> From<(Option<F>, S)> for EncodingConfigWithFraming
where
    F: Into<FramingConfig>,
    S: Into<SerializerConfig>,
{
    fn from((framing, encoding): (Option<F>, S)) -> Self {
        Self {
            framing: framing.map(Into::into),
            encoding: encoding.into().into(),
        }
    }
}

#[cfg(test)]
mod test {
    use lookup::lookup_v2::parse_value_path_old;

    use super::*;
    use crate::codecs::encoding::TimestampFormat;

    #[test]
    fn deserialize_encoding_config() {
        let string = r#"
            {
                "codec": "json",
                "only_fields": ["a.b[0]"],
                "except_fields": ["ignore_me"],
                "timestamp_format": "unix"
            }
        "#;

        let encoding = serde_json::from_str::<EncodingConfig>(string).unwrap();
        let serializer = encoding.config();

        assert!(matches!(serializer, SerializerConfig::Json));

        let transformer = encoding.transformer();

        assert_eq!(
            transformer.only_fields(),
            &Some(vec![parse_value_path_old("a.b[0]")])
        );
        assert_eq!(
            transformer.except_fields(),
            &Some(vec!["ignore_me".to_owned()])
        );
        assert_eq!(transformer.timestamp_format(), &Some(TimestampFormat::Unix));
    }

    #[test]
    fn deserialize_encoding_config_with_framing() {
        let string = r#"
            {
                "framing": {
                    "method": "newline_delimited"
                },
                "encoding": {
                    "codec": "json",
                    "only_fields": ["a.b[0]"],
                    "except_fields": ["ignore_me"],
                    "timestamp_format": "unix"
                }
            }
        "#;

        let encoding = serde_json::from_str::<EncodingConfigWithFraming>(string).unwrap();
        let (framing, serializer) = encoding.config();

        assert!(matches!(framing, Some(FramingConfig::NewlineDelimited)));
        assert!(matches!(serializer, SerializerConfig::Json));

        let transformer = encoding.transformer();

        assert_eq!(
            transformer.only_fields(),
            &Some(vec![parse_value_path_old("a.b[0]")])
        );
        assert_eq!(
            transformer.except_fields(),
            &Some(vec!["ignore_me".to_owned()])
        );
        assert_eq!(transformer.timestamp_format(), &Some(TimestampFormat::Unix));
    }

    #[test]
    fn deserialize_encoding_config_without_framing() {
        let string = r#"
            {
                "encoding": {
                    "codec": "json",
                    "only_fields": ["a.b[0]"],
                    "except_fields": ["ignore_me"],
                    "timestamp_format": "unix"
                }
            }
        "#;

        let encoding = serde_json::from_str::<EncodingConfigWithFraming>(string).unwrap();
        let (framing, serializer) = encoding.config();

        assert!(matches!(framing, None));
        assert!(matches!(serializer, SerializerConfig::Json));

        let transformer = encoding.transformer();

        assert_eq!(
            transformer.only_fields(),
            &Some(vec![parse_value_path_old("a.b[0]")])
        );
        assert_eq!(
            transformer.except_fields(),
            &Some(vec!["ignore_me".to_owned()])
        );
        assert_eq!(transformer.timestamp_format(), &Some(TimestampFormat::Unix));
    }
}
