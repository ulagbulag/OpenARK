pub extern crate k8s_openapi;
pub extern crate kube;
pub extern crate serde_json;
pub extern crate serde_yaml;

pub mod ansible;
pub mod cluster;
pub mod r#box;
pub mod manager;
pub mod proxy;

pub mod consts {
    pub const NAMESPACE: &'static str = "kiss";
}
