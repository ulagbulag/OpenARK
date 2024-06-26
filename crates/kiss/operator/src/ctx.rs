use std::{sync::Arc, time::Duration};

use anyhow::Result;
use ark_core_k8s::manager::Manager;
use async_trait::async_trait;
use chrono::Utc;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kiss_ansible::{AnsibleClient, AnsibleJob, AnsibleResourceType};
use kiss_api::r#box::{BoxCrd, BoxGroupRole, BoxState, BoxStatus};
use kube::{
    api::{Patch, PatchParams},
    runtime::controller::Action,
    Api, CustomResourceExt, Error, ResourceExt,
};
use serde_json::json;
use tracing::{info, instrument, warn, Level};

#[derive(Default)]
pub struct Ctx {}

#[async_trait]
impl ::ark_core_k8s::manager::Ctx for Ctx {
    type Data = BoxCrd;

    const NAME: &'static str = crate::consts::NAME;
    const NAMESPACE: &'static str = ::kiss_api::consts::NAMESPACE;

    fn get_subcrds() -> Vec<CustomResourceDefinition> {
        vec![
            ::kiss_api::netbox::NetBoxCrd::crd(),
            ::kiss_api::rack::RackCrd::crd(),
        ]
    }

    #[instrument(level = Level::INFO, skip_all, fields(name = %data.name_any(), namespace = data.namespace()), err(Display))]
    async fn reconcile(
        manager: Arc<Manager<Self>>,
        data: Arc<<Self as ::ark_core_k8s::manager::Ctx>::Data>,
    ) -> Result<Action, Error>
    where
        Self: Sized,
    {
        let crd = BoxCrd::api_resource();
        let name = data.name_any();
        let status = data.status.as_ref();
        let api = Api::<<Self as ::ark_core_k8s::manager::Ctx>::Data>::all(manager.kube.clone());

        // get the current time
        let now = Utc::now();

        // load the box's state
        let old_state = status
            .as_ref()
            .map(|status| status.state)
            .unwrap_or(BoxState::New);
        let mut new_state = old_state.next();
        let mut new_group = None;

        // detect the box's group is changed
        let is_bind_group_updated = status
            .as_ref()
            .and_then(|status| status.bind_group.as_ref())
            .map(|bind_group| bind_group != &data.spec.group)
            .unwrap_or(true);

        // wait new boxes with no access methods for begin provisioned
        if matches!(old_state, BoxState::New)
            && !status
                .as_ref()
                .map(|status| status.access.primary.is_some())
                .unwrap_or_default()
        {
            let timeout = BoxState::timeout_new();

            if let Some(last_updated) = data.last_updated() {
                if now > *last_updated + timeout {
                    // update the status
                    new_state = BoxState::Disconnected;
                } else {
                    return Ok(Action::requeue(timeout.to_std().unwrap()));
                }
            } else {
                return Ok(Action::requeue(timeout.to_std().unwrap()));
            }
        }

        // capture the timeout
        if let Some(time_threshold) = old_state.timeout() {
            if let Some(last_updated) = data.last_updated() {
                if now > *last_updated + time_threshold {
                    // update the status
                    new_state = BoxState::Failed;
                }
            }
        }

        // capture the group info is changed
        if matches!(old_state, BoxState::Running) && is_bind_group_updated {
            new_state = BoxState::Disconnected;
        }

        // load kiss config
        let ansible = match AnsibleClient::try_default(&manager.kube).await {
            Ok(ansible) => ansible,
            Err(e) => {
                warn!("failed to create AnsibleClient: {e}");
                return Ok(Action::requeue(
                    <Self as ::ark_core_k8s::manager::Ctx>::FALLBACK,
                ));
            }
        };

        if !matches!(old_state, BoxState::Joining) && matches!(new_state, BoxState::Joining) {
            // skip joining to default cluster as worker nodes when external
            if matches!(data.spec.group.role, BoxGroupRole::ExternalWorker) {
                info!("Skipped joining (box is external) {name:?}");
                return Ok(Action::requeue(
                    <Self as ::ark_core_k8s::manager::Ctx>::FALLBACK,
                ));
            }

            // skip joining to default cluster as worker nodes when disabled
            if !ansible.kiss.group_enable_default_cluster
                && data.spec.group.is_default()
                && matches!(data.spec.group.role, BoxGroupRole::GenericWorker)
            {
                info!("Skipped joining (default cluster is disabled) {name:?}");
                return Ok(Action::requeue(
                    <Self as ::ark_core_k8s::manager::Ctx>::FALLBACK,
                ));
            }

            // skip joining if already joined
            if !is_bind_group_updated {
                let patch = Patch::Merge(json!({
                    "apiVersion": crd.api_version,
                    "kind": crd.kind,
                    "status": BoxStatus {
                        access: status.map(|status| status.access.clone()).unwrap_or_default(),
                        state: BoxState::Running,
                        bind_group: status.and_then(|status| status.bind_group.clone()),
                        last_updated: Utc::now(),
                    },
                }));
                let pp = PatchParams::apply(Self::NAME);
                api.patch_status(&name, &pp, &patch).await?;

                info!("Skipped joining (already joined) {name:?}");
                return Ok(Action::requeue(
                    <Self as ::ark_core_k8s::manager::Ctx>::FALLBACK,
                ));
            }

            // bind to new group
            new_group = Some(&data.spec.group);
        }

        // spawn an Ansible job
        if old_state != new_state || new_state.cron().is_some() {
            if let Some(task) = new_state.as_task() {
                let is_spawned = ansible
                    .spawn(
                        &manager.kube,
                        AnsibleJob {
                            cron: new_state.cron(),
                            task,
                            r#box: &data,
                            new_group,
                            new_state: Some(new_state),
                            is_critical: false,
                            resource_type: match old_state {
                                BoxState::New
                                | BoxState::Commissioning
                                | BoxState::Ready
                                | BoxState::Joining => AnsibleResourceType::Normal,
                                BoxState::Running
                                | BoxState::GroupChanged
                                | BoxState::Failed
                                | BoxState::Disconnected => AnsibleResourceType::Minimal,
                            },
                            use_workers: false,
                        },
                    )
                    .await?;

                // If there is a problem spawning a job, check back after a few minutes
                if !is_spawned {
                    info!("Cannot spawn an Ansible job; waiting: {}", &name);
                    return Ok(Action::requeue(
                        #[allow(clippy::identity_op)]
                        Duration::from_secs(1 * 60),
                    ));
                }
            }

            // wait for being changed
            if old_state == new_state {
                info!("Waiting for being changed: {name:?}");
                return Ok(Action::await_change());
            }

            // bind group before joining to a cluster
            let bind_group = if matches!(new_state, BoxState::Joining) {
                Some(&data.spec.group)
            } else {
                status
                    .as_ref()
                    .and_then(|status| status.bind_group.as_ref())
            };

            let patch = Patch::Merge(json!({
                "apiVersion": crd.api_version,
                "kind": crd.kind,
                "status": BoxStatus {
                    access: status.map(|status| status.access.clone()).unwrap_or_default(),
                    state: new_state,
                    bind_group: bind_group.cloned(),
                    last_updated: Utc::now(),
                },
            }));
            let pp = PatchParams::apply(Self::NAME);
            api.patch_status(&name, &pp, &patch).await?;

            info!("Reconciled Document {name:?}");
        }

        // If no events were received, check back after a few minutes
        Ok(Action::requeue(
            <Self as ::ark_core_k8s::manager::Ctx>::FALLBACK,
        ))
    }
}
