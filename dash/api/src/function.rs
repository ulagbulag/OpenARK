use ipis::core::chrono::{DateTime, Utc};
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

use crate::model::ModelFieldsSpec;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, CustomResource)]
#[kube(
    group = "dash.ulagbulag.io",
    version = "v1alpha1",
    kind = "Function",
    struct = "FunctionCrd",
    status = "FunctionStatus",
    shortname = "f",
    printcolumn = r#"{
        "name": "state",
        "type": "string",
        "description":"state of the function",
        "jsonPath":".status.state"
    }"#,
    printcolumn = r#"{
        "name": "created-at",
        "type": "date",
        "description":"created time",
        "jsonPath":".metadata.creationTimestamp"
    }"#
)]
#[serde(rename_all = "camelCase")]
pub struct FunctionSpec {
    pub input: ModelFieldsSpec,
    #[serde(default)]
    pub output: Option<ModelFieldsSpec>,
    pub actor: FunctionActorSpec,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FunctionStatus {
    pub state: Option<FunctionState>,
    pub last_updated: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum FunctionActorSpec {
    Job(FunctionActorJobSpec),
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum FunctionActorJobSpec {
    ConfigMapRef(FunctionActorSourceConfigMapRefSpec),
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FunctionActorSourceConfigMapRefSpec {
    pub name: String,
    pub namespace: String,
    pub path: String,
}

#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    EnumString,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub enum FunctionState {
    Pending,
    Ready,
}