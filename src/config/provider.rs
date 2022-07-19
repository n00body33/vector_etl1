use async_trait::async_trait;
use toml::Value;

use super::{component::ExampleError, GenerateConfig};
use crate::{providers, signal};

#[async_trait]
#[typetag::serde(tag = "type")]
pub trait ProviderConfig: core::fmt::Debug + Send + Sync + dyn_clone::DynClone {
    /// Builds a provider, returning a string containing the config. It's passed a signals
    /// channel to control reloading and shutdown, as applicable.
    async fn build(&mut self, signal_handler: &mut signal::SignalHandler) -> providers::Result;
    fn provider_type(&self) -> &'static str;
}

dyn_clone::clone_trait_object!(ProviderConfig);

/// Describes a provider plugin storing its type name and an optional example config.
pub struct ProviderDescription {
    pub type_str: &'static str,
    example_value: fn() -> Option<Value>,
}

impl ProviderDescription
where
    inventory::iter<ProviderDescription>:
        std::iter::IntoIterator<Item = &'static ProviderDescription>,
{
    /// Creates a new provider plugin description.
    /// Configuration example is generated by the `GenerateConfig` trait.
    pub const fn new<B: GenerateConfig>(type_str: &'static str) -> Self {
        Self {
            type_str,
            example_value: || Some(B::generate_config()),
        }
    }

    /// Returns an example config for a plugin identified by its type.
    pub fn example(type_str: &str) -> Result<Value, ExampleError> {
        inventory::iter::<ProviderDescription>
            .into_iter()
            .find(|t| t.type_str == type_str)
            .ok_or_else(|| ExampleError::DoesNotExist {
                type_str: type_str.to_owned(),
            })
            .and_then(|t| (t.example_value)().ok_or(ExampleError::MissingExample))
    }
}

inventory::collect!(ProviderDescription);
