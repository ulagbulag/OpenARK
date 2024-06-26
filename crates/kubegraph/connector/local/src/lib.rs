use std::{path::Path, sync::Arc};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::{stream::iter, StreamExt};
use kubegraph_api::{
    connector::{
        local::NetworkConnectorLocalSpec, NetworkConnectorCrd, NetworkConnectorKind,
        NetworkConnectorSpec, NetworkConnectorType,
    },
    frame::LazyFrame,
    graph::{Graph, GraphData, GraphMetadataRaw, GraphScope},
};
use polars::{
    frame::DataFrame,
    io::{csv::read::CsvReadOptions, SerReader},
    lazy::frame::IntoLazy,
};
use tokio::fs;
use tracing::{info, instrument, warn, Level};

#[derive(Default)]
pub struct NetworkConnector {}

#[async_trait]
impl ::kubegraph_api::connector::NetworkConnector for NetworkConnector {
    #[inline]
    fn connector_type(&self) -> NetworkConnectorType {
        NetworkConnectorType::Local
    }

    #[inline]
    fn name(&self) -> &str {
        "local"
    }

    #[instrument(level = Level::INFO, skip(self, connectors))]
    async fn pull(
        &mut self,
        connectors: Vec<NetworkConnectorCrd>,
    ) -> Result<Vec<Graph<GraphData<LazyFrame>>>> {
        let items = connectors.into_iter().filter_map(|object| {
            let cr = Arc::new(object.clone());
            let scope = GraphScope::from_resource(&object);
            let NetworkConnectorSpec { kind } = object.spec;

            match kind {
                NetworkConnectorKind::Local(spec) => Some(NetworkConnectorItem { cr, scope, spec }),
                _ => None,
            }
        });

        let data = iter(items).filter_map(|item| async move {
            let GraphScope { namespace, name } = item.scope.clone();
            match item.load_graph_data().await {
                Ok(data) => Some(data),
                Err(error) => {
                    warn!("failed to load local connector ({namespace}/{name}): {error}");
                    None
                }
            }
        });

        Ok(data.collect().await)
    }
}

#[derive(Clone, Debug)]
struct NetworkConnectorItem {
    cr: Arc<NetworkConnectorCrd>,
    scope: GraphScope,
    spec: NetworkConnectorLocalSpec,
}

impl NetworkConnectorItem {
    #[instrument(level = Level::INFO, skip(self))]
    async fn load_graph_data(self) -> Result<Graph<GraphData<LazyFrame>>> {
        let Self {
            cr,
            scope,
            spec:
                NetworkConnectorLocalSpec {
                    path: base_dir,
                    key_edges,
                    key_nodes,
                },
        } = self;

        let GraphScope { namespace, name } = &scope;
        info!("Loading local connector: {namespace}/{name}");

        let edges = load_csv(&base_dir, &key_edges).await?;
        let nodes = load_csv(&base_dir, &key_nodes).await?;

        let metadata = GraphMetadataRaw::from_polars(&nodes).into();

        Ok(Graph {
            connector: Some(cr.clone()),
            data: GraphData {
                edges: LazyFrame::Polars(edges.lazy()),
                nodes: LazyFrame::Polars(nodes.lazy()),
            },
            metadata,
            scope,
        })
    }
}

#[instrument(level = Level::INFO)]
async fn load_csv(base_dir: &Path, filename: &str) -> Result<DataFrame> {
    let mut path = base_dir.to_path_buf();
    path.push(filename);

    if fs::try_exists(&path).await? {
        CsvReadOptions::default()
            .with_has_header(true)
            .try_into_reader_with_file_path(Some(path.to_path_buf()))
            .map_err(
                |error| anyhow!("failed to load file {path}: {error}", path = path.display(),),
            )?
            .finish()
            .map_err(|error| {
                anyhow!(
                    "failed to parse file {path}: {error}",
                    path = path.display(),
                )
            })
    } else {
        Ok(DataFrame::default())
    }
}
