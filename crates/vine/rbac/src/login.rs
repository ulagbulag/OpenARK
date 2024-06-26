use anyhow::Result;
use kube::Client;
use tracing::{instrument, Level};
use vine_api::user_auth::UserSessionResponse;

#[instrument(level = Level::INFO, skip(client), err(Display))]
pub async fn execute(
    client: &Client,
    box_name: &str,
    user_name: &str,
    logout_on_failed: bool,
) -> Result<UserSessionResponse> {
    super::session::execute_with(
        client,
        box_name,
        user_name,
        true,
        |session_manager, spec| async move {
            session_manager
                .try_create(&spec.as_ref(), logout_on_failed)
                .await
        },
    )
    .await
}
