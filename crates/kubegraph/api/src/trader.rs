use std::collections::BTreeMap;

use anyhow::Result;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    function::NetworkFunctionCrd,
    graph::{GraphData, GraphEdges, GraphMetadataPinned, GraphScope},
    problem::VirtualProblem,
};

#[async_trait]
pub trait NetworkTrader<T> {
    async fn is_locked(&self, problem: &VirtualProblem) -> Result<bool>;

    async fn register(&self, ctx: NetworkTraderContext<T>) -> Result<()>;
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkTraderContext<T> {
    pub functions: BTreeMap<GraphScope, NetworkFunctionCrd>,
    pub graph: GraphData<T>,
    pub problem: VirtualProblem<GraphMetadataPinned>,
    pub static_edges: Option<GraphEdges<T>>,
}