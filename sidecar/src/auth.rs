//! Bearer authentication for loopback management endpoints.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use subtle::ConstantTimeEq;

#[derive(Clone)]
pub struct AuthState {
    token: Arc<Vec<u8>>,
}

impl AuthState {
    pub fn new(token: &str) -> Self {
        Self {
            token: Arc::new(token.as_bytes().to_vec()),
        }
    }

    pub fn matches(&self, presented: &[u8]) -> bool {
        let expected = self.token.as_slice();
        if expected.len() != presented.len() {
            return false;
        }
        bool::from(expected.ct_eq(presented))
    }
}

pub async fn require_bearer(
    State(auth): State<AuthState>,
    request: Request,
    next: Next,
) -> Response {
    let authorized = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(|token| auth.matches(token.as_bytes()))
        .unwrap_or(false);

    if authorized {
        next.run(request).await
    } else {
        (StatusCode::UNAUTHORIZED, "unauthorized").into_response()
    }
}
