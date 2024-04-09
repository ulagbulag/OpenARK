use std::{borrow::Cow, collections::BTreeSet, net::IpAddr};

use anyhow::{bail, Result};
use itertools::Itertools;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kiss_api::r#box::{BoxCrd, BoxGroupRole, BoxGroupSpec, BoxSpec, BoxState};
use kube::{api::ListParams, Api, Client, Error};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument, Level};
use uuid::Uuid;

use crate::config::KissConfig;

pub struct ClusterState<'a> {
    config: Cow<'a, KissConfig>,
    control_planes: ClusterControlPlaneState,
    owner_group: Cow<'a, BoxGroupSpec>,
    owner_uuid: Uuid,
}

impl<'a> ClusterState<'a> {
    #[instrument(level = Level::INFO, skip(kube, config, owner), err(Display))]
    pub async fn load(
        kube: &'a Client,
        config: &'a KissConfig,
        owner: &'a BoxSpec,
    ) -> Result<ClusterState<'a>, Error> {
        Ok(Self {
            config: Cow::Borrowed(config),
            control_planes: ClusterControlPlaneState::load(kube, &owner.group.cluster_name).await?,
            owner_group: Cow::Borrowed(&owner.group),
            owner_uuid: owner.machine.uuid,
        })
    }

    #[instrument(level = Level::INFO, skip(kube), err(Display))]
    pub async fn load_current_cluster(kube: &'a Client) -> Result<ClusterState<'a>> {
        let config = KissConfig::try_default(kube).await?;
        let cluster_name = config.kiss_cluster_name.clone();

        // load the current control planes
        let control_planes = ClusterControlPlaneState::load(kube, &cluster_name).await?;

        // load a master node
        let filter = ClusterControlPlaneFilter::Running;
        let owner = match control_planes.iter(filter).next() {
            Some(owner) => owner.clone(),
            None => bail!("cluster {cluster_name:?} is not running"),
        };

        Ok(Self {
            config: Cow::Owned(config),
            control_planes,
            owner_group: Cow::Owned(BoxGroupSpec {
                cluster_name,
                role: BoxGroupRole::ControlPlane,
            }),
            owner_uuid: owner.uuid,
        })
    }

    pub fn get_control_planes_as_string(&self) -> String {
        let filter = ClusterControlPlaneFilter::RunningWith {
            uuid: self.owner_uuid,
        };
        let nodes = self.control_planes.to_vec(filter);

        const NODE_ROLE: &str = "kube_control_plane";
        get_nodes_as_string(nodes, NODE_ROLE)
    }

    pub fn get_etcd_nodes_as_string(&self) -> String {
        let filter = ClusterControlPlaneFilter::RunningWith {
            uuid: self.owner_uuid,
        };
        let mut nodes = self.control_planes.to_vec(filter);

        // estimate the number of default nodes
        let num_default_nodes = if self.owner_group.is_default() {
            self.config.bootstrapper_node_size
        } else {
            0
        };

        // truncate the number of nodes to `etcd_nodes_max`
        if self.config.etcd_nodes_max > 0 {
            nodes.truncate(if self.config.etcd_nodes_max < num_default_nodes {
                0
            } else {
                self.config.etcd_nodes_max - num_default_nodes
            });
        }

        // ETCD nodes should be odd (RAFT)
        if (nodes.len() + num_default_nodes) % 2 == 0 {
            nodes.pop();
        }

        const NODE_ROLE: &str = "etcd";
        get_nodes_as_string(nodes, NODE_ROLE)
    }

    pub fn is_control_plane_ready(&self) -> bool {
        let control_planes_running = self.control_planes.num_running();
        let control_planes_total = self.control_planes.num_total();

        info!(
            "Cluster \"{}\" status: {}/{} control-plane nodes are ready",
            &self.owner_group.cluster_name, control_planes_running, control_planes_total,
        );

        // test the count of nodes
        if control_planes_running == 0 && !self.owner_group.is_default() {
            info!(
                "Cluster \"{}\" status: no control-plane nodes are defined",
                &self.owner_group.cluster_name,
            );
            return false;
        }

        // assert all control plane nodes are ready
        control_planes_running == control_planes_total
    }

    fn is_node_control_plane(&self) -> bool {
        self.control_planes.contains(self.owner_uuid)
    }

    pub fn is_joinable(&self) -> bool {
        if self.is_node_control_plane() {
            self.control_planes.is_next(self.owner_uuid)
        } else {
            self.is_control_plane_ready()
        }
    }

    pub fn is_new(&self) -> bool {
        self.is_node_control_plane() && !self.control_planes.is_running()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ClusterBoxState {
    created_at: Option<Time>,
    hostname: String,
    ip: Option<IpAddr>,
    is_running: bool,
    uuid: Uuid,
}

impl AsRef<Self> for ClusterBoxState {
    fn as_ref(&self) -> &Self {
        self
    }
}

impl ClusterBoxState {
    fn get_host(&self) -> Option<String> {
        Some(format!("{}:{}", &self.hostname, self.ip?))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ClusterLockState {
    box_name: String,
    role: BoxGroupRole,
}

struct ClusterControlPlaneState {
    nodes: BTreeSet<ClusterBoxState>,
}

impl ClusterControlPlaneState {
    #[instrument(level = Level::INFO, err(Display), skip(kube))]
    async fn load(kube: &Client, cluster_name: &str) -> Result<Self, Error> {
        let api = Api::<BoxCrd>::all(kube.clone());
        let lp = ListParams::default();
        Ok(Self {
            nodes: api
                .list(&lp)
                .await?
                .items
                .into_iter()
                .filter(|r#box| {
                    r#box.spec.group.cluster_name == cluster_name
                        && r#box.spec.group.role == BoxGroupRole::ControlPlane
                })
                .map(|r#box| ClusterBoxState {
                    created_at: r#box.metadata.creation_timestamp.clone(),
                    hostname: r#box.spec.machine.hostname(),
                    ip: r#box
                        .status
                        .as_ref()
                        .and_then(|status| status.access.primary.as_ref())
                        .map(|interface| interface.address),
                    is_running: r#box
                        .status
                        .as_ref()
                        .map(|status| matches!(status.state, BoxState::Running))
                        .unwrap_or_default(),
                    uuid: r#box.spec.machine.uuid,
                })
                .collect(),
        })
    }

    fn contains(&self, uuid: Uuid) -> bool {
        self.nodes.iter().any(|node| node.uuid == uuid)
    }

    fn is_next(&self, uuid: Uuid) -> bool {
        self.nodes
            .iter()
            .find(|node| !node.is_running)
            .map(|node| node.uuid == uuid)
            .unwrap_or_default()
    }

    fn is_running(&self) -> bool {
        self.nodes.iter().any(|node| node.is_running)
    }

    fn iter(&self, filter: ClusterControlPlaneFilter) -> impl Iterator<Item = &ClusterBoxState> {
        self.nodes
            .iter()
            .filter(|node| node.is_running || filter.contains(node.uuid))
            .sorted_by_key(|&node| (&node.created_at, node))
    }

    fn num_running(&self) -> usize {
        self.nodes.iter().filter(|node| node.is_running).count()
    }

    fn num_total(&self) -> usize {
        self.nodes.len()
    }

    fn to_vec(&self, filter: ClusterControlPlaneFilter) -> Vec<&ClusterBoxState> {
        self.iter(filter).collect()
    }
}

enum ClusterControlPlaneFilter {
    Running,
    RunningWith { uuid: Uuid },
}

impl ClusterControlPlaneFilter {
    fn contains(&self, target: Uuid) -> bool {
        match self {
            Self::Running => false,
            Self::RunningWith { uuid } => *uuid == target,
        }
    }
}

fn get_nodes_as_string(nodes: Vec<&ClusterBoxState>, node_role: &str) -> String {
    nodes
        .iter()
        .sorted_by_key(|&&node| {
            (
                // Place the unready node to the last
                // so that the cluster info should be preferred.
                !node.is_running,
                &node.created_at,
                node,
            )
        })
        .filter_map(|node| node.get_host())
        .map(|host| format!("{node_role}:{host}"))
        .join(" ")
}
