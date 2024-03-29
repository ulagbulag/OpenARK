use anyhow::Result;
use dash_api::model_claim::{ModelClaimCrd, ModelClaimDeletionPolicy, ModelClaimState};
use dash_provider::storage::KubernetesStorageClient;
use kube::ResourceExt;
use tracing::{instrument, Level};

pub struct ModelClaimValidator<'namespace, 'kube> {
    pub kubernetes_storage: KubernetesStorageClient<'namespace, 'kube>,
}

impl<'namespace, 'kube> ModelClaimValidator<'namespace, 'kube> {
    #[instrument(level = Level::INFO, skip_all, err(Display))]
    pub async fn validate_model_claim(
        &self,
        field_manager: &str,
        crd: &ModelClaimCrd,
    ) -> Result<ModelClaimState> {
        // create model
        let model = self
            .kubernetes_storage
            .load_model_or_create_as_dynamic(field_manager, &crd.name_any())
            .await?;

        // check model is already binded
        if !self
            .kubernetes_storage
            .load_model_storage_bindings(&model.name_any())
            .await?
            .is_empty()
        {
            return Ok(ModelClaimState::Ready);
        }

        // notify the model claim
        // TODO: to be implemented!
        Ok(ModelClaimState::Pending)
    }

    #[instrument(level = Level::INFO, skip_all, err(Display))]
    pub async fn delete(&self, crd: &ModelClaimCrd) -> Result<()> {
        match crd.status.as_ref().and_then(|status| status.spec.as_ref()) {
            Some(spec) => match spec.deletion_policy {
                ModelClaimDeletionPolicy::Delete => self.delete_model(crd).await,
                ModelClaimDeletionPolicy::Retain => Ok(()),
            },
            None => Ok(()),
        }
    }

    #[instrument(level = Level::INFO, skip_all, err(Display))]
    async fn delete_model(&self, crd: &ModelClaimCrd) -> Result<()> {
        self.kubernetes_storage.delete_model(&crd.name_any()).await
    }
}
