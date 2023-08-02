use std::{collections::HashMap, net::SocketAddr};
use base64::Engine;
use base64::prelude::BASE64_URL_SAFE;

use bytes::Bytes;
use itertools::Itertools;
use vector_config::configurable_component;
use vector_core::config::LogNamespace;
use warp::http::HeaderMap;

use super::parser;
use crate::{
    config::{
        GenerateConfig, SourceAcknowledgementsConfig, SourceConfig, SourceContext, SourceOutput,
    },
    event::Event,
    serde::bool_or_struct,
    sources::{
        self,
        util::{http::HttpMethod, ErrorMessage, HttpSource, HttpSourceAuthConfig},
    },
    tls::TlsEnableableConfig,
};


/// Configuration for the `prometheus_pushgateway` source.
#[configurable_component(source(
    "prometheus_pushgateway",
    "Receive metrics via the Prometheus Pushgateway protocol."
))]
#[derive(Clone, Debug)]
pub struct PrometheusPushgatewayConfig {
    /// The socket address to accept connections on.
    ///
    /// The address _must_ include a port.
    #[configurable(metadata(docs::examples = "0.0.0.0:9091"))]
    address: SocketAddr,

    #[configurable(derived)]
    tls: Option<TlsEnableableConfig>,

    #[configurable(derived)]
    #[configurable(metadata(docs::advanced))]
    auth: Option<HttpSourceAuthConfig>,

    #[configurable(derived)]
    #[serde(default, deserialize_with = "bool_or_struct")]
    acknowledgements: SourceAcknowledgementsConfig,

    // TODO: Add toggle for whether to aggregate counters and histograms
}

impl GenerateConfig for PrometheusPushgatewayConfig {
    fn generate_config() -> toml::Value {
        toml::Value::try_from(Self {
            address: "127.0.0.1:9091".parse().unwrap(),
            tls: None,
            auth: None,
            acknowledgements: SourceAcknowledgementsConfig::default(),
        })
        .unwrap()
    }
}

#[async_trait::async_trait]
#[typetag::serde(name = "prometheus_pushgateway")]
impl SourceConfig for PrometheusPushgatewayConfig {
    async fn build(&self, cx: SourceContext) -> crate::Result<sources::Source> {
        let source = PushgatewaySource;
        source.run(
            self.address,
            // TODO: Support configuring path so we can run multiple of these
            "",
            HttpMethod::Post,
            false,
            &self.tls,
            &self.auth,
            cx,
            self.acknowledgements,
        )
    }

    fn outputs(&self, _global_log_namespace: LogNamespace) -> Vec<SourceOutput> {
        vec![SourceOutput::new_metrics()]
    }

    fn can_acknowledge(&self) -> bool {
        true
    }
}

#[derive(Clone)]
struct PushgatewaySource;

// impl PushgatewaySource {
//     fn decode_body(&self, _body: Bytes) -> Result<Vec<Event>, ErrorMessage> {
//         let mut result = Vec::new();
//
//         let counter = Metric::new(
//             "foo",
//             MetricKind::Absolute,
//             MetricValue::Counter {
//                 value: 49.0,
//             },
//         );
//
//         result.push(counter.into());
//
//         Ok(result)
//     }
// }

impl HttpSource for PushgatewaySource {
    fn build_events(
        &self,
        body: Bytes,
        _header_map: &HeaderMap,
        _query_parameters: &HashMap<String, String>,
        full_path: &str,
    ) -> Result<Vec<Event>, ErrorMessage> {
        let body = String::from_utf8_lossy(&body);

        println!("Full path was: {}", full_path);
        let path_labels = parse_path_labels(full_path)?.into_iter().collect();

        // TODO: Add grouping key to these
        // TODO: Add an option to toggle between incremental and absolute, default to absolute
        match parser::parse_text_with_overrides(&body, path_labels) {
            Ok(events) => {
                Ok(events)
            },
            Err(_error) => {
                Ok(vec![])
            }
        }
    }
}

fn parse_path_labels(path: &str) -> Result<Vec<(String,String)>,ErrorMessage> {
    let pairs = path.split('/')
        // Skip the first two segments as they're the empty string and
        // "metrics", which is always there as a path prefix
        .skip(2)
        .chunks(2)
        .into_iter()
        // If we get a chunk that only has 1 item, return an error
        // The path has to be made up of key-value pairs to be valid
        .map(|mut c|
            c.next().zip(c.next()).ok_or_else(|| {
                ErrorMessage::new(
                    http::StatusCode::BAD_REQUEST,
                    "Request path must have an even number of segments to form grouping key".to_string())
            })
        )
        // Decode any values that have been base64 encoded per the Pushgateway spec
        //
        // See: https://github.com/prometheus/pushgateway#url
        .map(|res|
            res.and_then(|(k,v)|
                decode_label_pair(k, v)
            )
        )
        .collect();

    println!("{:?}", pairs);

    pairs
}

fn decode_label_pair(k: &str, v: &str) -> Result<(String, String), ErrorMessage> {
    if !k.ends_with("@base64") {
        return Ok((k.to_owned(), v.to_owned()))
    }

    let decoded_bytes = BASE64_URL_SAFE.decode(v).map_err( |_|
        ErrorMessage::new(
            http::StatusCode::BAD_REQUEST,
            format!("Invalid base64 value for key {}", k))
    )?;

    let decoded = String::from_utf8(decoded_bytes).map_err( |_|
        ErrorMessage::new(
        http::StatusCode::BAD_REQUEST,
        format!("Invalid UTF-8 in base64 value for key {}", k))
    )?;

    Ok((k.to_owned(), decoded))
}
