use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, JsonSchema)]
pub struct Flags {
    #[serde(default)]
    pub require_ndc_capabilities: bool,
}
