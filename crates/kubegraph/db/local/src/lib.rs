use anyhow::{anyhow, Result};
use async_trait::async_trait;
use clap::Parser;
use kubegraph_api::graph::{NetworkEntry, NetworkEntryKeyFilter, NetworkEntryMap};
use serde::{Deserialize, Serialize};
use sled::{Batch, Config, Db};
use tracing::{info, instrument, Level};

#[derive(Clone, Debug, Serialize, Deserialize, Parser)]
pub struct NetworkGraphDBArgs {
    #[arg(
        long,
        env = "KUBEGRAPH_DB_PATH",
        value_name = "PATH",
        default_value_t = NetworkGraphDBArgs::default_db_path(),
    )]
    #[serde(default = "NetworkGraphDBArgs::default_db_path")]
    db_path: String,
}

impl NetworkGraphDBArgs {
    pub fn default_db_path() -> String {
        "default.sled".into()
    }
}

#[derive(Clone)]
pub struct NetworkGraphDB {
    db: Db,
}

impl NetworkGraphDB {
    #[instrument(level = Level::INFO)]
    pub async fn try_default() -> Result<Self> {
        let args = NetworkGraphDBArgs::try_parse()?;
        Self::try_new(&args).await
    }

    #[instrument(level = Level::INFO, skip(args))]
    pub async fn try_new(args: &NetworkGraphDBArgs) -> Result<Self> {
        info!("Loading local db...");

        let NetworkGraphDBArgs { db_path } = args;

        Ok(Self {
            db: Config::default()
                .path(db_path)
                .open()
                .map_err(|error| anyhow!("failed to open db: {error}"))?,
        })
    }
}

#[async_trait]
impl ::kubegraph_api::db::NetworkGraphDB for NetworkGraphDB {
    #[instrument(level = Level::INFO, skip_all)]
    async fn add_entries(
        &self,
        entries: impl Send + IntoIterator<Item = NetworkEntry>,
    ) -> Result<()> {
        let mut batch = Batch::default();

        entries
            .into_iter()
            .filter_map(|NetworkEntry { key, value }| {
                let key = ::serde_json::to_vec(&key).ok()?;
                let value = ::serde_json::to_vec(&value).ok()?;
                Some((key, value))
            })
            .for_each(|(key, value)| {
                batch.insert(key, value);
            });

        self.db
            .apply_batch(batch)
            .map_err(|error| anyhow!("failed to write edges: {error}"))
    }

    #[instrument(level = Level::INFO, skip_all)]
    async fn get_entries(&self, filter: Option<&NetworkEntryKeyFilter>) -> NetworkEntryMap {
        self.db
            .iter()
            .filter_map(|result| result.ok())
            .filter_map(|(key, value)| {
                let key = ::serde_json::from_slice(&key).ok()?;
                let value = ::serde_json::from_slice(&value).ok()?;
                Some((key, value))
            })
            .filter(|(key, _)| filter.map(|filter| filter.contains(key)).unwrap_or(true))
            .map(|(key, value)| NetworkEntry { key, value })
            .fold(NetworkEntryMap::default(), |mut map, entry| {
                map.push(entry);
                map
            })
    }

    async fn close(self) -> Result<()> {
        info!("Closing local db...");

        self.db
            .flush_async()
            .await
            .map(|_| ())
            .map_err(|error| anyhow!("failed to flush local db: {error}"))
    }
}
