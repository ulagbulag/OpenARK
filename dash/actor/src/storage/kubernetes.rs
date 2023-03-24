use dash_api::{
    function::{FunctionActorSourceConfigMapRefSpec, FunctionCrd, FunctionState},
    k8s_openapi::{
        api::core::v1::ConfigMap,
        apiextensions_apiserver::pkg::apis::apiextensions::v1::{
            CustomResourceDefinition, CustomResourceDefinitionVersion,
        },
    },
    kube::{
        api::ListParams,
        core::{object::HasStatus, DynamicObject},
        discovery, Api, Client, ResourceExt,
    },
    model::{ModelCrd, ModelCustomResourceDefinitionRefSpec, ModelState},
    model_storage_binding::{ModelStorageBindingCrd, ModelStorageBindingState},
    serde_json::Value,
    storage::{ModelStorageCrd, ModelStorageSpec, ModelStorageState},
};
use ipis::{
    core::anyhow::{bail, Result},
    itertools::Itertools,
};

#[derive(Copy, Clone)]
pub struct KubernetesStorageClient<'a> {
    pub kube: &'a Client,
}

impl<'a> KubernetesStorageClient<'a> {
    pub async fn load_config_map<'f>(
        &self,
        spec: &'f FunctionActorSourceConfigMapRefSpec,
    ) -> Result<(&'f str, String)> {
        let FunctionActorSourceConfigMapRefSpec {
            name,
            namespace,
            path,
        } = spec;

        let api = Api::<ConfigMap>::namespaced(self.kube.clone(), namespace);
        let config_map = api.get(name).await?;

        match config_map.data.and_then(|mut data| data.remove(path)) {
            Some(content) => Ok((path, content)),
            None => bail!("no such file in ConfigMap: {path:?} in {namespace}::{name}"),
        }
    }

    pub async fn load_custom_resource(
        &self,
        spec: &ModelCustomResourceDefinitionRefSpec,
        namespace: &str,
        resource_name: &str,
    ) -> Result<Option<Value>> {
        let (api_group, scope, def) = self.load_custom_resource_definition(spec).await?;

        // Discover most stable version variant of document
        let apigroup = discovery::group(self.kube, &api_group).await?;
        let ar = match apigroup.versioned_resources(&def.name).pop() {
            Some((ar, _)) => ar,
            None => {
                let model_name = &spec.name;
                bail!("no such CRD: {model_name:?}")
            }
        };

        // Use the discovered kind in an Api, and Controller with the ApiResource as its DynamicType
        let api: Api<DynamicObject> = match scope.as_str() {
            "Namespaced" => Api::namespaced_with(self.kube.clone(), namespace, &ar),
            "Cluster" => Api::all_with(self.kube.clone(), &ar),
            scope => bail!("cannot infer CRD scope {scope:?}: {resource_name:?}"),
        };
        Ok(api.get_opt(resource_name).await?.map(|object| object.data))
    }

    pub async fn load_custom_resource_definition(
        &self,
        spec: &ModelCustomResourceDefinitionRefSpec,
    ) -> Result<(String, String, CustomResourceDefinitionVersion)> {
        let (api_group, version) = crate::imp::parse_api_version(&spec.name)?;

        let api = Api::<CustomResourceDefinition>::all(self.kube.clone());
        let crd = api.get(api_group).await?;

        match crd.spec.versions.iter().find(|def| def.name == version) {
            Some(def) => Ok((crd.spec.group, crd.spec.scope, def.clone())),
            None => bail!(
                "CRD version is invalid; expected one of {:?}, but given {version}",
                crd.spec.versions.iter().map(|def| &def.name).join(","),
            ),
        }
    }

    pub async fn load_model(&self, name: &str) -> Result<ModelCrd> {
        let api = Api::<ModelCrd>::all(self.kube.clone());
        let model = api.get(name).await?;

        match &model.status {
            Some(status) if status.state == Some(ModelState::Ready) => match &status.fields {
                Some(_) => Ok(model),
                None => bail!("model has no fields status: {name:?}"),
            },
            Some(_) | None => bail!("model is not ready: {name:?}"),
        }
    }

    pub async fn load_model_all(&self) -> Result<Vec<String>> {
        let api = Api::<ModelCrd>::all(self.kube.clone());
        let lp = ListParams::default();
        let models = api.list(&lp).await?;

        Ok(models
            .into_iter()
            .filter(|model| {
                model
                    .status()
                    .map(|status| {
                        matches!(status.state, Some(ModelState::Ready)) && status.fields.is_some()
                    })
                    .unwrap_or_default()
            })
            .map(|model| model.name_any())
            .collect())
    }

    pub async fn load_model_storage(&self, name: &str) -> Result<ModelStorageCrd> {
        let api = Api::<ModelStorageCrd>::all(self.kube.clone());
        let storage = api.get(name).await?;

        match &storage.status {
            Some(status) if status.state == Some(ModelStorageState::Ready) => Ok(storage),
            Some(_) | None => bail!("model storage is not ready: {name:?}"),
        }
    }

    pub async fn load_model_storage_bindings(
        &self,
        model_name: &str,
    ) -> Result<Vec<ModelStorageSpec>> {
        let api = Api::<ModelStorageBindingCrd>::all(self.kube.clone());
        let lp = ListParams::default();
        let bindings = api.list(&lp).await?;

        Ok(bindings
            .items
            .into_iter()
            .filter(|binding| {
                binding
                    .status()
                    .and_then(|status| status.state)
                    .map(|state| matches!(state, ModelStorageBindingState::Ready))
                    .unwrap_or_default()
            })
            .filter(|binding| binding.spec.model == model_name)
            .filter_map(|binding| binding.status.unwrap().storage)
            .collect())
    }

    pub async fn load_function(&self, name: &str) -> Result<FunctionCrd> {
        let api = Api::<FunctionCrd>::all(self.kube.clone());
        let function = api.get(name).await?;

        match &function.status {
            Some(status) if status.state == Some(FunctionState::Ready) => match &status.spec {
                Some(_) => Ok(function),
                None => bail!("function has no spec status: {name:?}"),
            },
            Some(_) | None => bail!("function is not ready: {name:?}"),
        }
    }

    pub async fn load_function_all(&self) -> Result<Vec<String>> {
        let api = Api::<FunctionCrd>::all(self.kube.clone());
        let lp = ListParams::default();
        let functions = api.list(&lp).await?;

        Ok(functions
            .into_iter()
            .filter(|function| {
                function
                    .status()
                    .map(|status| {
                        matches!(status.state, Some(FunctionState::Ready)) && status.spec.is_some()
                    })
                    .unwrap_or_default()
            })
            .map(|function| function.name_any())
            .collect())
    }
}
