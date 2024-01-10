use indexmap::IndexMap;
// use ndc_client::models::SchemaResponse;
use crate::ndc_client::models::SchemaResponse;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::SecretValue;

use super::{
    ndc_schema_response_schema_reference, CapabilitiesResponseWithSchema, DataConnectorName,
};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "ReadWriteUrlsV2")]
pub struct ReadWriteUrlsV2 {
    pub read: SecretValue,
    pub write: SecretValue,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[schemars(title = "DataConnectorUrlV2")]
#[serde(rename_all = "camelCase")]
pub enum DataConnectorUrlV2 {
    SingleUrl(SecretValue),
    ReadWriteUrls(ReadWriteUrlsV2),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "DataConnectorV2")]
pub struct DataConnectorV2 {
    pub name: DataConnectorName,
    pub url: DataConnectorUrlV2,
    #[serde(default)]
    /// Key value map of HTTP headers to be sent with each request to the data connector
    pub headers: IndexMap<String, SecretValue>,
    #[serde(default)]
    #[schemars(schema_with = "ndc_schema_response_schema_reference")]
    pub schema: SchemaResponse,
    pub capabilities: Option<CapabilitiesResponseWithSchema>,
}
