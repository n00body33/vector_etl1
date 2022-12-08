#[cfg(all(test, feature = "opentelemetry-integration-tests"))]
mod integration_tests;
#[cfg(test)]
mod tests;

mod grpc;
mod http;
mod reply;
mod status;

use std::net::SocketAddr;

use futures::{future::join, FutureExt, TryFutureExt};
use lookup::owned_value_path;
use opentelemetry_proto::convert::{
    ATTRIBUTES_KEY, DROPPED_ATTRIBUTES_COUNT_KEY, FLAGS_KEY, OBSERVED_TIMESTAMP_KEY, RESOURCE_KEY,
    SEVERITY_NUMBER_KEY, SEVERITY_TEXT_KEY, SPAN_ID_KEY, TRACE_ID_KEY,
};

use opentelemetry_proto::proto::collector::logs::v1::logs_service_server::LogsServiceServer;
use value::kind::Collection;
use value::Kind;
use vector_common::internal_event::{BytesReceived, Protocol};
use vector_config::{configurable_component, NamedComponent};
use vector_core::config::LegacyKey;
use vector_core::{config::LogNamespace, schema::Definition};

use crate::{
    config::{
        DataType, GenerateConfig, Output, Resource, SourceAcknowledgementsConfig, SourceConfig,
        SourceContext,
    },
    serde::bool_or_struct,
    sources::{util::grpc::run_grpc_server, Source},
    tls::{MaybeTlsSettings, TlsEnableableConfig},
};

use self::{
    grpc::Service,
    http::{build_warp_filter, run_http_server},
};

pub const LOGS: &str = "logs";

/// Configuration for the `opentelemetry` source.
#[configurable_component(source("opentelemetry"))]
#[derive(Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct OpentelemetryConfig {
    #[configurable(derived)]
    grpc: GrpcConfig,

    #[configurable(derived)]
    http: HttpConfig,

    #[configurable(derived)]
    #[serde(default, deserialize_with = "bool_or_struct")]
    acknowledgements: SourceAcknowledgementsConfig,

    /// The namespace to use for logs. This overrides the global setting.
    #[configurable(metadata(docs::hidden))]
    #[serde(default)]
    log_namespace: Option<bool>,
}

/// Configuration for the `opentelemetry` gRPC server.
#[configurable_component]
#[derive(Clone, Debug)]
#[serde(deny_unknown_fields)]
struct GrpcConfig {
    /// The address to listen for connections on.
    ///
    /// It _must_ include a port.
    address: SocketAddr,

    #[configurable(derived)]
    #[serde(default)]
    tls: Option<TlsEnableableConfig>,
}

/// Configuration for the `opentelemetry` HTTP server.
#[configurable_component]
#[derive(Clone, Debug)]
#[serde(deny_unknown_fields)]
struct HttpConfig {
    /// The address to listen for connections on.
    ///
    /// It _must_ include a port.
    address: SocketAddr,

    #[configurable(derived)]
    #[serde(default)]
    tls: Option<TlsEnableableConfig>,
}

impl GenerateConfig for OpentelemetryConfig {
    fn generate_config() -> toml::Value {
        toml::Value::try_from(Self {
            grpc: GrpcConfig {
                address: "0.0.0.0:4317".parse().unwrap(),
                tls: Default::default(),
            },
            http: HttpConfig {
                address: "0.0.0.0:4318".parse().unwrap(),
                tls: Default::default(),
            },
            acknowledgements: Default::default(),
            log_namespace: None,
        })
        .unwrap()
    }
}

#[async_trait::async_trait]
impl SourceConfig for OpentelemetryConfig {
    async fn build(&self, cx: SourceContext) -> crate::Result<Source> {
        let acknowledgements = cx.do_acknowledgements(self.acknowledgements);
        let log_namespace = cx.log_namespace(self.log_namespace);

        let grpc_tls_settings = MaybeTlsSettings::from_config(&self.grpc.tls, true)?;
        let grpc_service = LogsServiceServer::new(Service {
            pipeline: cx.out.clone(),
            acknowledgements,
            log_namespace,
        })
        .accept_compressed(tonic::codec::CompressionEncoding::Gzip);
        let grpc_source = run_grpc_server(
            self.grpc.address,
            grpc_tls_settings,
            grpc_service,
            cx.shutdown.clone(),
        )
        .map_err(|error| {
            error!(message = "Source future failed.", %error);
        });

        let http_tls_settings = MaybeTlsSettings::from_config(&self.http.tls, true)?;
        let protocol = http_tls_settings.http_protocol_name();
        let bytes_received = register!(BytesReceived::from(Protocol::from(protocol)));
        let filters = build_warp_filter(acknowledgements, log_namespace, cx.out, bytes_received);
        let http_source =
            run_http_server(self.http.address, http_tls_settings, filters, cx.shutdown);

        Ok(join(grpc_source, http_source).map(|_| Ok(())).boxed())
    }

    fn outputs(&self, global_log_namespace: LogNamespace) -> Vec<Output> {
        let log_namespace = global_log_namespace.merge(self.log_namespace);
        // TODO `.` should have meaning "message" when LogNamespace::Vector
        let schema_definition = Definition::new_with_default_metadata(Kind::any(), [log_namespace])
            .with_source_metadata(
                Self::NAME,
                Some(LegacyKey::Overwrite(owned_value_path!(RESOURCE_KEY))),
                &owned_value_path!(RESOURCE_KEY),
                Kind::object(Collection::from_unknown(Kind::any())).or_undefined(),
                None,
            )
            .with_source_metadata(
                Self::NAME,
                Some(LegacyKey::Overwrite(owned_value_path!(ATTRIBUTES_KEY))),
                &owned_value_path!(ATTRIBUTES_KEY),
                Kind::object(Collection::from_unknown(Kind::any())).or_undefined(),
                None,
            )
            .with_source_metadata(
                Self::NAME,
                Some(LegacyKey::Overwrite(owned_value_path!(TRACE_ID_KEY))),
                &owned_value_path!(TRACE_ID_KEY),
                Kind::bytes().or_undefined(),
                None,
            )
            .with_source_metadata(
                Self::NAME,
                Some(LegacyKey::Overwrite(owned_value_path!(SPAN_ID_KEY))),
                &owned_value_path!(SPAN_ID_KEY),
                Kind::bytes().or_undefined(),
                None,
            )
            .with_source_metadata(
                Self::NAME,
                Some(LegacyKey::Overwrite(owned_value_path!(SEVERITY_TEXT_KEY))),
                &owned_value_path!(SEVERITY_TEXT_KEY),
                Kind::bytes().or_undefined(),
                None,
            )
            .with_source_metadata(
                Self::NAME,
                Some(LegacyKey::Overwrite(owned_value_path!(SEVERITY_NUMBER_KEY))),
                &owned_value_path!(SEVERITY_NUMBER_KEY),
                Kind::integer().or_undefined(),
                None,
            )
            .with_source_metadata(
                Self::NAME,
                Some(LegacyKey::Overwrite(owned_value_path!(FLAGS_KEY))),
                &owned_value_path!(FLAGS_KEY),
                Kind::integer().or_undefined(),
                None,
            )
            .with_source_metadata(
                Self::NAME,
                Some(LegacyKey::Overwrite(owned_value_path!(
                    DROPPED_ATTRIBUTES_COUNT_KEY
                ))),
                &owned_value_path!(DROPPED_ATTRIBUTES_COUNT_KEY),
                Kind::integer(),
                None,
            )
            .with_source_metadata(
                Self::NAME,
                Some(LegacyKey::Overwrite(owned_value_path!(
                    OBSERVED_TIMESTAMP_KEY
                ))),
                &owned_value_path!(OBSERVED_TIMESTAMP_KEY),
                Kind::timestamp(),
                None,
            )
            .with_source_metadata(
                Self::NAME,
                None,
                &owned_value_path!("timestamp"),
                Kind::timestamp(),
                Some("timestamp"),
            )
            .with_standard_vector_source_metadata();

        vec![Output::default(DataType::Log)
            .with_port(LOGS)
            .with_schema_definition(schema_definition)]
    }

    fn resources(&self) -> Vec<Resource> {
        vec![
            Resource::tcp(self.grpc.address),
            Resource::tcp(self.http.address),
        ]
    }

    fn can_acknowledge(&self) -> bool {
        true
    }
}
