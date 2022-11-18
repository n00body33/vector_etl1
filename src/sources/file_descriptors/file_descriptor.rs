use std::fs::File;
use std::io;
use std::os::unix::io::FromRawFd;

use super::FileDescriptorConfig;
use codecs::decoding::{DeserializerConfig, FramingConfig};
use indoc::indoc;
use lookup::owned_value_path;
use value::Kind;
use vector_config::{configurable_component, NamedComponent};
use vector_core::config::{log_schema, LegacyKey, LogNamespace};

use crate::{
    config::{GenerateConfig, Output, Resource, SourceConfig, SourceContext},
    serde::default_decoding,
};
/// Configuration for the `file_descriptor` source.
#[configurable_component(source("file_descriptor"))]
#[derive(Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct FileDescriptorSourceConfig {
    /// The maximum buffer size, in bytes, of incoming messages.
    ///
    /// Messages larger than this are truncated.
    #[serde(default = "crate::serde::default_max_length")]
    pub max_length: usize,

    /// Overrides the name of the log field used to add the current hostname to each event.
    ///
    /// The value will be the current hostname for wherever Vector is running.
    ///
    /// By default, the [global `host_key` option](https://vector.dev/docs/reference/configuration//global-options#log_schema.host_key) is used.
    pub host_key: Option<String>,

    #[configurable(derived)]
    pub framing: Option<FramingConfig>,

    #[configurable(derived)]
    #[serde(default = "default_decoding")]
    pub decoding: DeserializerConfig,

    /// The file descriptor number to read from.
    pub fd: u32,

    /// The namespace to use for logs. This overrides the global setting.
    #[configurable(metadata(docs::hidden))]
    #[serde(default)]
    log_namespace: Option<bool>,
}

impl FileDescriptorConfig for FileDescriptorSourceConfig {
    fn host_key(&self) -> Option<String> {
        self.host_key.clone()
    }

    fn framing(&self) -> Option<FramingConfig> {
        self.framing.clone()
    }

    fn decoding(&self) -> DeserializerConfig {
        self.decoding.clone()
    }

    fn description(&self) -> String {
        format!("file descriptor {}", self.fd)
    }
}

impl GenerateConfig for FileDescriptorSourceConfig {
    fn generate_config() -> toml::Value {
        toml::from_str(indoc! {r#"
            fd = 10
        "#})
        .unwrap()
    }
}

#[async_trait::async_trait]
impl SourceConfig for FileDescriptorSourceConfig {
    async fn build(&self, cx: SourceContext) -> crate::Result<crate::sources::Source> {
        let pipe = io::BufReader::new(unsafe { File::from_raw_fd(self.fd as i32) });
        let log_namespace = cx.log_namespace(self.log_namespace);

        self.source(pipe, cx.shutdown, cx.out, log_namespace)
    }

    fn outputs(&self, global_log_namespace: LogNamespace) -> Vec<Output> {
        let log_namespace = global_log_namespace.merge(self.log_namespace);

        let host_key_path = self.host_key.as_ref().map_or_else(
            || owned_value_path!(log_schema().host_key()),
            |x| owned_value_path!(x),
        );

        let schema_definition = self
            .decoding
            .schema_definition(log_namespace)
            .with_source_metadata(
                Self::NAME,
                Some(LegacyKey::InsertIfEmpty(host_key_path)),
                &owned_value_path!("host"),
                Kind::bytes(),
                None,
            )
            .with_standard_vector_source_metadata();

        vec![Output::default(self.decoding.output_type()).with_schema_definition(schema_definition)]
    }

    fn resources(&self) -> Vec<Resource> {
        vec![Resource::Fd(self.fd)]
    }

    fn can_acknowledge(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use nix::unistd::{close, pipe, write};

    use super::*;
    use crate::{
        config::log_schema,
        test_util::components::{
            assert_source_compliance, assert_source_error, COMPONENT_ERROR_TAGS, SOURCE_TAGS,
        },
        SourceSender,
    };
    use futures::StreamExt;

    #[test]
    fn generate_config() {
        crate::test_util::test_generate_config::<FileDescriptorSourceConfig>();
    }

    #[tokio::test]
    async fn file_descriptor_decodes_line() {
        assert_source_compliance(&SOURCE_TAGS, async {
            let (tx, rx) = SourceSender::new_test();
            let (read_fd, write_fd) = pipe().unwrap();
            let config = FileDescriptorSourceConfig {
                max_length: crate::serde::default_max_length(),
                host_key: Default::default(),
                framing: None,
                decoding: default_decoding(),
                fd: read_fd as u32,
                log_namespace: None,
            };

            let mut stream = rx;

            write(write_fd, b"hello world\nhello world again\n").unwrap();
            close(write_fd).unwrap();

            let context = SourceContext::new_test(tx, None);
            config.build(context).await.unwrap().await.unwrap();

            let event = stream.next().await;
            assert_eq!(
                Some("hello world".into()),
                event.map(|event| event.as_log()[log_schema().message_key()]
                    .to_string_lossy()
                    .into_owned())
            );

            let event = stream.next().await;
            assert_eq!(
                Some("hello world again".into()),
                event.map(|event| event.as_log()[log_schema().message_key()]
                    .to_string_lossy()
                    .into_owned())
            );

            let event = stream.next().await;
            assert!(event.is_none());
        })
        .await;
    }

    #[tokio::test]
    async fn file_descriptor_handles_invalid_fd() {
        assert_source_error(&COMPONENT_ERROR_TAGS, async {
            let (tx, rx) = SourceSender::new_test();
            let (_read_fd, write_fd) = pipe().unwrap();
            let config = FileDescriptorSourceConfig {
                max_length: crate::serde::default_max_length(),
                host_key: Default::default(),
                framing: None,
                decoding: default_decoding(),
                fd: write_fd as u32, // intentionally giving the source a write-only fd
                log_namespace: None,
            };

            let mut stream = rx;

            write(write_fd, b"hello world\nhello world again\n").unwrap();
            close(write_fd).unwrap();

            let context = SourceContext::new_test(tx, None);
            config.build(context).await.unwrap().await.unwrap();

            // The error "Bad file descriptor" will be logged when the source attempts to read
            // for the first time.
            let event = stream.next().await;
            assert!(event.is_none());
        })
        .await;
    }
}
