use anyhow::{anyhow, Result};
use ark_core_k8s::data::{EmailAddress, Url};
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{user::UserSpec, user_box_quota::UserBoxQuotaSpec};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, CustomResource)]
#[kube(
    group = "vine.ulagbulag.io",
    version = "v1alpha1",
    kind = "UserAuth",
    root = "UserAuthCrd",
    shortname = "ua",
    printcolumn = r#"{
        "name": "created-at",
        "type": "date",
        "description": "created time",
        "jsonPath": ".metadata.creationTimestamp"
    }"#,
    printcolumn = r#"{
        "name": "version",
        "type": "integer",
        "description": "user auth version",
        "jsonPath": ".metadata.generation"
    }"#
)]
#[serde(rename_all = "camelCase")]
pub enum UserAuthSpec {
    OIDC {
        #[serde(flatten)]
        oauth2: UserAuthOAuth2Common,
        issuer: Url,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserAuthLoginQuery {
    pub box_uuid: Uuid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserAuthOAuth2Common {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UserAuthPayload {
    /// User primary id
    #[serde(default, alias = "sub")]
    id: Option<String>,
    /// User e-mail address
    email: String,
    /// User name
    #[serde(default)]
    name: Option<String>,
    /// Preferred user name
    #[serde(alias = "preferred_username")]
    username: String,
    /// User roles
    #[serde(default)]
    roles: String,
}

impl UserAuthPayload {
    pub fn primary_key(&self) -> Result<String> {
        fn encode(s: &str) -> String {
            s.to_lowercase()
                // common special words
                .replace('-', "-s-")
                .replace('.', "-d-")
                .replace('@', "-at-")
                // other special words
                .replace('_', "-u-")
        }

        let id = || {
            let id = self.id.as_ref()?;
            Some(format!("id-{}", encode(id)))
        };
        let email = || {
            let email = self.email.parse::<EmailAddress>().ok()?;
            Some(format!("email-{}", encode(email.as_str())))
        };
        let name = || {
            let name = &self.username;
            if name.is_empty() {
                None
            } else {
                Some(format!("name-{}", encode(name)))
            }
        };

        name()
            .or_else(email)
            .or_else(id)
            .ok_or_else(|| anyhow!("failed to parse primary key: {:?}", self))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "status", content = "data")]
pub enum UserSessionResponse {
    Accept {
        box_quota: UserBoxQuotaSpec,
        user: UserSpec,
    },
    Error(UserSessionError),
}

impl From<UserAuthError> for UserSessionResponse {
    fn from(error: UserAuthError) -> Self {
        Self::Error(error.into())
    }
}

#[derive(Clone, Debug, Error, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "status", content = "data")]
pub enum UserSessionError {
    #[error("This user is already logged in to {node_name:?}")]
    AlreadyLoggedInByNode { node_name: String },
    #[error("This node is already logged in by {user_name:?}")]
    AlreadyLoggedInByUser { user_name: String },
    #[error("{0}")]
    AuthError(UserAuthError),
    #[error("This node is not registered. Please contact the administrator.")]
    NodeNotFound,
    #[error("This node is not permitted. Please contact the administrator.")]
    NodeNotInCluster,
    #[error("This node is reserved to other user.")]
    NodeReserved,
    #[error("This node does not meet quota requirements. Please contact the administrator.")]
    QuotaMismatched,
}

impl From<UserAuthError> for UserSessionError {
    fn from(error: UserAuthError) -> Self {
        Self::AuthError(error)
    }
}

#[derive(Clone, Debug, Error, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "status", content = "data")]
pub enum UserAuthError {
    #[error("Malformed authorization token. Please contact the administrator.")]
    AuthorizationTokenMalformed,
    #[error("Missing authorization token. Please contact the administrator.")]
    AuthorizationTokenNotFound,
    #[error("This user has no permission to sign in. Please contact the administrator.")]
    NamespaceNotAllowed,
    #[error("Missing namespace token. Please contact the administrator.")]
    NamespaceTokenMalformed,
    #[error("Malformed primary key. Please contact the administrator.")]
    PrimaryKeyMalformed,
    #[error("This user is not an admin. Please contact the administrator.")]
    UserNotAdmin,
    #[error("This user is not registered. Please contact the administrator.")]
    UserNotRegistered,
    #[error("This user has no available vine sessions. Please contact the administrator.")]
    SessionNotBinded,
}
