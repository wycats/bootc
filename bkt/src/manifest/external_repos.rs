//! External RPM repositories manifest types.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct ExternalReposManifest {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    pub repos: Vec<ExternalRepo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExternalRepo {
    pub name: String,
    pub display_name: String,
    pub baseurl: String,
    pub gpg_key: String,
    pub packages: Vec<String>,
}
