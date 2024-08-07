use actix_web::{
    post,
    web::{Data, Json},
    HttpRequest, HttpResponse, Responder,
};
use ark_core::result::Result;
use kube::Client;
use tracing::{instrument, warn, Level};
use vine_api::user_session::{UserSession, UserSessionCommand};
use vine_rbac::auth::{AuthUserSession, AuthUserSessionRef};
use vine_session::exec::SessionExecExt;

#[instrument(level = Level::INFO, skip(request, kube))]
#[post("/user/desktop/exec")]
pub async fn post_exec(
    request: HttpRequest,
    kube: Data<Client>,
    Json(command): Json<UserSessionCommand>,
) -> impl Responder {
    let kube = kube.as_ref().clone();
    let session = match UserSession::from_request(&kube, &request)
        .await
        .and_then(|session| session.try_into_ark_session())
    {
        Ok(session) => session,
        Err(error) => {
            warn!("{error}");
            return HttpResponse::from(Result::<()>::Err(error.to_string()));
        }
    };

    let result = session.exec_without_tty(kube, command).await.map(|_| ());
    HttpResponse::from(Result::from(result))
}
