use axum::{
    body::Body,
    extract::State,
    http::{header, Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

use crate::auth::bearer_token;
use crate::{AppState, WebSecurityConfig};

pub(crate) fn cors_layer(security: &WebSecurityConfig) -> CorsLayer {
    let layer = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]);
    if security.allowed_origins.is_empty() {
        layer
    } else {
        layer.allow_origin(security.allowed_origins.clone())
    }
}

pub(crate) async fn authorize_mutations(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let requires_auth = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) || request.uri().path().starts_with("/ws/collab/");
    if request.uri().path() == "/api/auth/login" {
        return next.run(request).await;
    }
    if !requires_auth {
        return next.run(request).await;
    }

    let supplied = bearer_token(request.headers());
    let static_token_matches = state
        .security
        .auth_token
        .as_deref()
        .is_some_and(|expected| {
            supplied.is_some_and(|value| constant_time_eq(value.as_bytes(), expected.as_bytes()))
        });
    let session_matches = match supplied {
        Some(token) if state.auth.has_users() => state.auth.authenticate(token).await.is_ok(),
        _ => false,
    };
    if static_token_matches
        || session_matches
        || (!state.auth.has_users() && state.security.auth_token.is_none())
    {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
            Json(serde_json::json!({ "error": "authorization required" })),
        )
            .into_response()
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |diff, (left, right)| diff | (left ^ right))
        == 0
}
