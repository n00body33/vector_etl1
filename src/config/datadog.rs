use super::{Config, SinkOuter, SourceOuter};
use crate::{
    sinks::datadog::metrics::DatadogConfig, sources::internal_metrics::InternalMetricsConfig,
};
use serde::{Deserialize, Serialize};
use std::env::var;

// The '#' character here is being used to denote an internal name. It's 'unspeakable'
// in default TOML configuration, but could clash in JSON config so this isn't fool-proof.
// TODO: Refactor for component scope once https://github.com/timberio/vector/pull/8654 lands.
static INTERNAL_METRICS_NAME: &'static str = "#datadog_internal_metrics";
static DATADOG_METRICS_NAME: &'static str = "#datadog_sink";

#[derive(Debug, Deserialize, Serialize, PartialEq, Copy, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub api_key: Option<String>,
}

/// By default, the Datadog feature is enabled where an API key is provided.
fn default_enabled() -> bool {
    true
}

/// Augment configuration with observability via Datadog if the feature is enabled and
/// an API key is provided.
pub fn attach(config: &mut Config) {
    /// Return early if an API key is missing, or the feature isn't enabled.
    let api_key = match (&config.datadog.api_key, config.datadog.enabled) {
        (Some(api_key), true) => api_key.clone(),
        _ => return,
    };

    info!("Datadog API key detected. Internal metrics will be sent to Datadog.");

    // Create an internal metrics source. We're using a distinct source here and not
    // attempting to reuse an existing one, due to the use of a custom namespace to
    // satisfy reporting to Datadog.
    let internal_metrics = InternalMetricsConfig::namespace("pipelines");

    config.sources.insert(
        INTERNAL_METRICS_NAME.to_string(),
        SourceOuter::new(internal_metrics),
    );

    // Create a Datadog metrics sink to consume and emit internal + host metrics.
    let datadog_metrics = DatadogConfig {
        api_key,
        ..DatadogConfig::default()
    };

    config.sinks.insert(
        DATADOG_METRICS_NAME.to_string(),
        SinkOuter::new(
            vec![INTERNAL_METRICS_NAME.to_string()],
            Box::new(datadog_metrics),
        ),
    );
}
