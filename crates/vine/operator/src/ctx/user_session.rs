use std::sync::Arc;

use anyhow::Result;
use ark_core_k8s::manager::Manager;
use async_trait::async_trait;
use k8s_openapi::api::core::v1::Node;
use kube::{runtime::controller::Action, Error, ResourceExt};
use tracing::{info, instrument, warn, Level};
use vine_api::user::UserCrd;
use vine_session::SessionManager;

#[derive(Default)]
pub struct Ctx {}

#[async_trait]
impl ::ark_core_k8s::manager::Ctx for Ctx {
    type Data = Node;

    const NAME: &'static str = crate::consts::NAME;
    const NAMESPACE: &'static str = ::vine_api::consts::NAMESPACE;

    #[instrument(level = Level::INFO, skip_all, fields(name = %data.name_any(), namespace = data.namespace()), err(Display))]
    async fn reconcile(
        manager: Arc<Manager<Self>>,
        data: Arc<<Self as ::ark_core_k8s::manager::Ctx>::Data>,
    ) -> Result<Action, Error>
    where
        Self: Sized,
    {
        let name = data.name_any();
        let namespace = match data.labels().get(::ark_api::consts::LABEL_BIND_BY_USER) {
            Some(user_name) => UserCrd::user_namespace_with(user_name),
            None => {
                info!("skipping unbinding node ({name}): user not found");
                return Ok(Action::requeue(
                    <Self as ::ark_core_k8s::manager::Ctx>::FALLBACK,
                ));
            }
        };

        if data
            .labels()
            .get(::ark_api::consts::LABEL_BIND_PERSISTENT)
            .map(|persistent| persistent == "true")
            .unwrap_or_default()
        {
            info!(
                "skipping unbinding node ({name}): node is allocated persistently to {namespace:?}"
            );
            return Ok(Action::requeue(
                <Self as ::ark_core_k8s::manager::Ctx>::FALLBACK,
            ));
        }

        let session_manager = match SessionManager::try_new(namespace, manager.kube.clone()).await {
            Ok(session_manager) => session_manager,
            Err(e) => {
                warn!("failed to create a SessionManager: {e}");
                return Ok(Action::requeue(
                    <Self as ::ark_core_k8s::manager::Ctx>::FALLBACK,
                ));
            }
        };

        match session_manager.try_delete(&data).await {
            Ok(Some(user_name)) => {
                info!("unbinded node: {name:?} => {user_name:?}");
            }
            Ok(None) => {}
            Err(e) => {
                warn!("failed to unbind node: {name:?}: {e}");
            }
        }

        // If no events were received, check back after a few minutes
        Ok(Action::requeue(
            <Self as ::ark_core_k8s::manager::Ctx>::FALLBACK,
        ))
    }
}
