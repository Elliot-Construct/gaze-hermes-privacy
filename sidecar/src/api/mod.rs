//! HTTP API surface.

pub mod status;

use axum::routing::get;
use axum::Router;

use crate::auth::{require_bearer, AuthState};

pub fn build_router(auth: AuthState) -> Router {
    let public = Router::new().route("/healthz", get(status::healthz));
    let protected = Router::new()
        .route("/v1/status", get(status::status))
        .layer(axum::middleware::from_fn_with_state(auth.clone(), require_bearer));
    public.merge(protected).with_state(auth)
}
