use super::EnrichmentTable;
use crate::config::{EnrichmentTableConfig, EnrichmentTableDescription};
use serde::{Deserialize, Serialize};
use tracing::trace;

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
struct FileConfig;

#[async_trait::async_trait]
#[typetag::serde(name = "file")]
impl EnrichmentTableConfig for FileConfig {
    async fn build(
        &self,
        _globals: &crate::config::GlobalOptions,
    ) -> crate::Result<Box<dyn super::EnrichmentTable + Send + Sync>> {
        trace!("Building file enrichment table");
        Ok(Box::new(File {
            data: vec![vec!["field1".to_string(), "field2".to_string()]],
            indexes: Vec::new(),
        }))
    }
}

inventory::submit! {
    EnrichmentTableDescription::new::<FileConfig>("file")
}

impl_generate_config_from_default!(FileConfig);

struct File {
    data: Vec<Vec<String>>,
    indexes: Vec<Vec<String>>,
}

impl EnrichmentTable for File {
    fn find_table_row<'a>(
        &self,
        _criteria: std::collections::BTreeMap<String, String>,
    ) -> Option<&Vec<String>> {
        trace!("Searching enrichment table.");
        Some(&self.data[0])
    }

    fn add_index(&mut self, fields: Vec<&str>) {
        self.indexes
            .push(fields.iter().map(ToString::to_string).collect());
    }
}

impl std::fmt::Debug for File {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "File {} row(s) {} index(es)",
            self.data.len(),
            self.indexes.len()
        )
    }
}
