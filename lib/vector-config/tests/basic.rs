// We allow dead code because some of the things we're testing are meant to ensure that the macros do the right thing
// for codegen i.e. not doing codegen for fields that `serde` is going to skip, etc.
#![allow(dead_code)]
#![allow(clippy::print_stdout)] // tests
#![allow(clippy::print_stderr)] // tests

use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    num::NonZeroU64,
    path::PathBuf,
    time::Duration,
};

use serde::{de, Deserialize, Deserializer};
use serde_with::serde_as;
use vector_config::{configurable_component, schema::generate_root_schema};

/// A templated string.
#[configurable_component]
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
#[serde(try_from = "String", into = "String")]
pub struct Template {
    /// The template string.
    src: String,

    #[serde(skip)]
    has_ts: bool,

    #[serde(skip)]
    has_fields: bool,
}

impl TryFrom<String> for Template {
    type Error = String;

    fn try_from(src: String) -> Result<Self, Self::Error> {
        if src.is_empty() {
            Err("wahhh".to_string())
        } else {
            Ok(Self {
                src,
                has_ts: false,
                has_fields: false,
            })
        }
    }
}

impl From<Template> for String {
    fn from(template: Template) -> String {
        template.src
    }
}

/// A period of time.
#[derive(Clone)]
#[configurable_component]
pub struct SpecialDuration(#[configurable(transparent)] u64);

/// Duration, but in seconds.
#[serde_as]
#[configurable_component]
#[derive(Clone)]
struct DurationSecondsTest {
    /// The timeout.
    #[serde_as(as = "serde_with::DurationSeconds<u64>")]
    timeout: Duration,
}

/// Controls the batching behavior of events.
#[derive(Clone)]
#[configurable_component]
#[serde(default)]
pub struct BatchConfig {
    /// The maximum number of events in a batch before it is flushed.
    #[configurable(validation(range(max = 100000)))]
    max_events: Option<NonZeroU64>,
    /// The maximum number of bytes in a batch before it is flushed.
    max_bytes: Option<NonZeroU64>,
    /// The maximum amount of time a batch can exist before it is flushed.
    timeout: Option<SpecialDuration>,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_events: Some(NonZeroU64::new(1000).expect("must be nonzero")),
            max_bytes: None,
            timeout: Some(SpecialDuration(10)),
        }
    }
}

/// The encoding to decode/encode events with.
#[derive(Clone)]
#[configurable_component]
#[serde(tag = "t", content = "c")]
pub enum Encoding {
    /// Text encoding.
    Text,
    /// JSON encoding.
    Json {
        /// Whether or not to render the output in a "pretty" form.
        ///
        /// If enabled, this will generally cause the output to be spread across more lines, with
        /// more indentation, resulting in an easy-to-read form for humans.  The opposite of this
        /// would be the standard output, which eschews whitespace for the most succient output.
        pretty: bool,
    },
    #[configurable(description = "MessagePack encoding.")]
    MessagePack(
        /// Starting offset for fields something something this is a fake description anyways.
        u64,
    ),
}

/// Enableable TLS configuration.
#[derive(Clone)]
#[configurable_component]
pub struct TlsEnablableConfig {
    /// Whether or not TLS is enabled.
    pub enabled: bool,
    #[serde(flatten)]
    pub options: TlsConfig,
}

/// TLS configuration.
#[derive(Clone)]
#[configurable_component]
pub struct TlsConfig {
    /// Certificate file.
    pub crt_file: Option<PathBuf>,
    /// Private key file.
    pub key_file: Option<PathBuf>,
}

/// A listening address that can optionally support being passed in by systemd.
#[derive(Clone, Copy, Debug, PartialEq)]
#[configurable_component]
#[serde(untagged)]
pub enum SocketListenAddr {
    /// A literal socket address.
    SocketAddr(#[configurable(derived)] SocketAddr),

    /// A file descriptor identifier passed by systemd.
    #[serde(deserialize_with = "parse_systemd_fd")]
    SystemdFd(#[configurable(transparent)] usize),
}

fn parse_systemd_fd<'de, D>(des: D) -> Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    let s: &'de str = Deserialize::deserialize(des)?;
    match s {
        "systemd" => Ok(0),
        s if s.starts_with("systemd#") => s[8..]
            .parse::<usize>()
            .map_err(de::Error::custom)?
            .checked_sub(1)
            .ok_or_else(|| de::Error::custom("systemd indices start from 1, found 0")),
        _ => Err(de::Error::custom("must start with \"systemd\"")),
    }
}

/// A source for collecting events over TCP.
#[derive(Clone)]
#[configurable_component(source)]
#[configurable(metadata(status = "beta"))]
pub struct SimpleSourceConfig {
    /// The address to listen on for events.
    #[serde(default = "default_simple_source_listen_addr")]
    listen_addr: SocketListenAddr,
    /*
    /// The timeout for waiting for events from the source before closing the source.
    #[serde(with = "DurationSeconds")]
    timeout: Duration,
    */
}

fn default_simple_source_listen_addr() -> SocketListenAddr {
    SocketListenAddr::SocketAddr(SocketAddr::V4(SocketAddrV4::new(
        Ipv4Addr::new(127, 0, 0, 1),
        9200,
    )))
}

/// A sink for sending events to the `simple` service.
#[derive(Clone)]
#[configurable_component(sink)]
#[configurable(metadata(status = "beta"))]
pub struct SimpleSinkConfig {
    /// The endpoint to send events to.
    #[serde(default = "default_simple_sink_endpoint")]
    endpoint: String,
    #[configurable(derived)]
    #[serde(default = "default_simple_sink_batch")]
    batch: BatchConfig,
    #[configurable(derived)]
    #[serde(default = "default_simple_sink_encoding")]
    encoding: Encoding,
    /// The filepath to write the events to.
    #[configurable(metadata(templateable))]
    output_path: Template,
    /// The tags to apply to each event.
    #[configurable(validation(length(max = 32)))]
    tags: HashMap<String, String>,
    #[serde(skip)]
    meaningless_field: String,
}

fn default_simple_sink_batch() -> BatchConfig {
    BatchConfig {
        max_events: Some(NonZeroU64::new(10000).expect("must be nonzero")),
        max_bytes: Some(NonZeroU64::new(16_000_000).expect("must be nonzero")),
        timeout: Some(SpecialDuration(5)),
    }
}

const fn default_simple_sink_encoding() -> Encoding {
    Encoding::Json { pretty: true }
}

fn default_simple_sink_endpoint() -> String {
    String::from("https://zalgo.io")
}

/// A sink for sending events to the `advanced` service.
#[derive(Clone)]
#[configurable_component(sink)]
#[configurable(metadata(status = "stable"))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct AdvancedSinkConfig {
    /// The endpoint to send events to.
    #[serde(default = "default_advanced_sink_endpoint")]
    endpoint: String,
    /// The agent version to simulate when sending events to the downstream service.
    ///
    /// Must match the pattern of "v\d+\.\d+\.\d+", which allows for values such as `v1.23.0` or `v0.1.3`, and so on.
    #[configurable(validation(pattern = "foo"))]
    agent_version: String,
    #[configurable(derived)]
    #[serde(default = "default_advanced_sink_batch")]
    batch: BatchConfig,
    #[configurable(deprecated, derived)]
    #[serde(default = "default_advanced_sink_encoding")]
    encoding: Encoding,
    /// Overridden TLS description.
    #[configurable(derived)]
    tls: Option<TlsEnablableConfig>,
    /// The partition key to use for each event.
    #[configurable(metadata(templateable))]
    #[serde(default = "default_partition_key")]
    partition_key: String,
    /// The tags to apply to each event.
    tags: HashMap<String, String>,
}

fn default_advanced_sink_batch() -> BatchConfig {
    BatchConfig {
        max_events: Some(NonZeroU64::new(5678).expect("must be nonzero")),
        max_bytes: Some(NonZeroU64::new(36_000_000).expect("must be nonzero")),
        timeout: Some(SpecialDuration(15)),
    }
}

fn default_partition_key() -> String {
    "foo".to_string()
}

const fn default_advanced_sink_encoding() -> Encoding {
    Encoding::Json { pretty: true }
}

fn default_advanced_sink_endpoint() -> String {
    String::from("https://zalgohtml5.io")
}

pub mod vector_v1 {
    use vector_config::configurable_component;

    use crate::SocketListenAddr;

    /// Configuration for version one of the `vector` source.
    #[configurable_component]
    #[derive(Clone, Debug)]
    #[serde(deny_unknown_fields)]
    pub(crate) struct VectorConfig {
        /// The address to listen for connections on.
        ///
        /// It _must_ include a port.
        address: SocketListenAddr,

        /// The timeout, in seconds, before a connection is forcefully closed during shutdown.
        #[serde(default = "default_shutdown_timeout_secs")]
        shutdown_timeout_secs: u64,

        /// The size, in bytes, of the receive buffer used for each connection.
        ///
        /// This should not typically needed to be changed.
        receive_buffer_bytes: Option<usize>,
    }

    const fn default_shutdown_timeout_secs() -> u64 {
        30
    }
}

pub mod vector_v2 {
    use std::net::SocketAddr;

    use vector_config::configurable_component;

    /// Configuration for version two of the `vector` source.
    #[configurable_component]
    #[derive(Clone, Debug)]
    #[serde(deny_unknown_fields)]
    pub struct VectorConfig {
        /// The address to listen for connections on.
        ///
        /// It _must_ include a port.
        pub address: SocketAddr,

        /// The timeout, in seconds, before a connection is forcefully closed during shutdown.
        #[serde(default = "default_shutdown_timeout_secs")]
        pub shutdown_timeout_secs: u64,
    }

    const fn default_shutdown_timeout_secs() -> u64 {
        30
    }
}

/// Marker type for the version one of the configuration for the `vector` source.
#[configurable_component]
#[derive(Clone, Debug)]
enum V1 {
    /// Marker value for version one.
    #[serde(rename = "1")]
    V1,
}

/// Configuration for version two of the `vector` source.
#[configurable_component]
#[derive(Clone, Debug)]
pub struct VectorConfigV1 {
    /// Version of the configuration.
    version: V1,

    #[serde(flatten)]
    config: self::vector_v1::VectorConfig,
}

/// Marker type for the version two of the configuration for the `vector` source.
#[configurable_component]
#[derive(Clone, Debug)]
enum V2 {
    /// Marker value for version two.
    #[serde(rename = "2")]
    V2,
}

/// Configuration for version two of the `vector` source.
#[configurable_component]
#[derive(Clone, Debug)]
pub struct VectorConfigV2 {
    /// Version of the configuration.
    version: Option<V2>,

    #[serde(flatten)]
    config: self::vector_v2::VectorConfig,
}

/// Configurable for the `vector` source.
#[configurable_component(source)]
#[derive(Clone, Debug)]
#[serde(untagged)]
pub enum VectorSourceConfig {
    /// Configuration for version one.
    V1(#[configurable(derived)] VectorConfigV1),

    /// Configuration for version two.
    V2(#[configurable(derived)] VectorConfigV2),
}

/// Collection of various sources available in Vector.
#[derive(Clone)]
#[configurable_component]
#[serde(tag = "type")]
pub enum SourceConfig {
    /// Simple source.
    Simple(#[configurable(derived)] SimpleSourceConfig),

    /// Vector source.
    Vector(#[configurable(derived)] VectorSourceConfig),
}

/// Collection of various sinks available in Vector.
#[derive(Clone)]
#[configurable_component]
#[serde(tag = "type")]
pub enum SinkConfig {
    /// Simple sink.
    Simple(#[configurable(derived)] SimpleSinkConfig),

    /// Advanced sink.
    Advanced(#[configurable(derived)] AdvancedSinkConfig),
}

#[derive(Clone)]
#[configurable_component]
#[configurable(description = "Global options for configuring Vector.")]
pub struct GlobalOptions {
    /// The data directory where Vector will store state.
    data_dir: Option<String>,
}

/// The overall configuration for Vector.
#[derive(Clone)]
#[configurable_component]
pub struct VectorConfig {
    #[configurable(derived)]
    global: GlobalOptions,
    /// Any configured sources.
    sources: Vec<SourceConfig>,
    /// Any configured sinks.
    sinks: Vec<SinkConfig>,
}

#[test]
fn vector_config() {
    let root_schema = generate_root_schema::<VectorConfig>();
    let json = serde_json::to_string_pretty(&root_schema)
        .expect("rendering root schema to JSON should not fail");

    println!("{}", json);
}
