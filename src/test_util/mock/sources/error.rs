use async_trait::async_trait;
use futures_util::{future::err, FutureExt};
use vector_config::configurable_component;
use vector_core::config::LogNamespace;
use vector_core::{
    config::{DataType, Output},
    source::Source,
};

use crate::config::{SourceConfig, SourceContext};

/// Configuration for the `test_error` source.
#[configurable_component(source("test_error"))]
#[derive(Clone, Debug, Default)]
pub struct ErrorSourceConfig {
    /// Meaningless field that only exists for triggering config diffs during topology reloading.
    data: Option<String>,
}

impl_generate_config_from_default!(ErrorSourceConfig);

#[async_trait]
impl SourceConfig for ErrorSourceConfig {
    async fn build(&self, _cx: SourceContext) -> crate::Result<Source> {
        Ok(err(()).boxed())
    }

    fn outputs(&self, _global_log_namespace: LogNamespace) -> Vec<Output> {
        vec![Output::default(DataType::Log)]
    }

    fn source_type(&self) -> &'static str {
        "test_error"
    }

    fn can_acknowledge(&self) -> bool {
        false
    }
}
