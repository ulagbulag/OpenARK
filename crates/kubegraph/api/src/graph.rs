use std::fmt;

use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEntry {
    #[serde(flatten)]
    pub key: NetworkEntrykey,
    pub value: NetworkValue,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum NetworkEntrykey {
    Edge(NetworkEdgeKey),
    Node(NetworkNodeKey),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEdge {
    #[serde(flatten)]
    pub key: NetworkEdgeKey,
    pub value: NetworkValue,
}

#[derive(
    Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(
    rename_all = "camelCase",
    bound = "
    NodeKey: Ord + Serialize + DeserializeOwned,
"
)]
pub struct NetworkEdgeKey<NodeKey = NetworkNodeKey>
where
    NodeKey: Ord,
{
    #[serde(default, rename = "le", skip_serializing_if = "Option::is_none")]
    pub interval_ms: Option<u64>,
    #[serde(
        flatten,
        deserialize_with = "self::prefix::link::deserialize",
        serialize_with = "self::prefix::link::serialize"
    )]
    pub link: NodeKey,
    #[serde(
        flatten,
        deserialize_with = "self::prefix::sink::deserialize",
        serialize_with = "self::prefix::sink::serialize"
    )]
    pub sink: NodeKey,
    #[serde(
        flatten,
        deserialize_with = "self::prefix::src::deserialize",
        serialize_with = "self::prefix::src::serialize"
    )]
    pub src: NodeKey,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkNode {
    #[serde(flatten)]
    pub key: NetworkNodeKey,
    pub value: NetworkValue,
}

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct NetworkNodeKey {
    pub kind: String,
    pub name: String,
    pub namespace: String,
}

impl fmt::Display for NetworkNodeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            kind,
            name,
            namespace,
        } = self;

        write!(f, "{kind}/{namespace}/{name}")
    }
}

#[derive(
    Copy, Clone, Debug, Default, PartialEq, PartialOrd, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct NetworkValue(pub f64);

mod prefix {
    ::serde_with::with_prefix!(pub(super) link "link_");
    ::serde_with::with_prefix!(pub(super) sink "sink_");
    ::serde_with::with_prefix!(pub(super) src "src_");
}
