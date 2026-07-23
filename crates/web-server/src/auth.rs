use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::AppState;

const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(8 * 60 * 60);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UserRole {
    Engineer,
    ElectricalEngineer,
    CommissioningEngineer,
    SafetyReviewer,
    ReleaseApprover,
    Admin,
}

impl UserRole {
    pub(crate) fn can_sign(self, hold_type: &str) -> bool {
        match self {
            Self::Admin => true,
            Self::ElectricalEngineer => matches!(hold_type, "wiring_review"),
            Self::CommissioningEngineer => {
                matches!(hold_type, "point_check_completion" | "hil_review")
            }
            Self::SafetyReviewer => matches!(hold_type, "safety_review"),
            Self::ReleaseApprover => matches!(hold_type, "release_approval"),
            Self::Engineer => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct AuthUser {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) role: UserRole,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct LoginRequest {
    pub(crate) username: String,
    pub(crate) password: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LoginResponse {
    pub(crate) token: String,
    pub(crate) expires_at_ms: u64,
    pub(crate) user: AuthUser,
}

#[derive(Debug, Clone, Deserialize)]
struct ConfiguredUser {
    username: String,
    password: String,
    #[serde(default)]
    display_name: Option<String>,
    role: UserRole,
}

#[derive(Debug, Clone)]
struct UserCredential {
    user: AuthUser,
    username: String,
    password: String,
}

#[derive(Debug, Clone)]
struct AuthSession {
    user: AuthUser,
    expires_at_ms: u64,
}

#[derive(Clone)]
pub(crate) struct AuthService {
    users: Arc<Vec<UserCredential>>,
    sessions: Arc<RwLock<HashMap<String, AuthSession>>>,
    session_ttl: Duration,
}

impl AuthService {
    pub(crate) fn from_env(allow_local_demo: bool) -> Result<Self, String> {
        let configured = match std::env::var("RUSTPLC_WEB_USERS_JSON") {
            Ok(raw) if !raw.trim().is_empty() => serde_json::from_str::<Vec<ConfiguredUser>>(&raw)
                .map_err(|err| format!("RUSTPLC_WEB_USERS_JSON is invalid: {err}"))?,
            _ if allow_local_demo => demo_users(),
            _ => Vec::new(),
        };

        if configured.iter().any(|user| {
            user.username.trim().is_empty()
                || user.password.is_empty()
                || user
                    .display_name
                    .as_deref()
                    .is_some_and(|name| name.trim().is_empty())
        }) {
            return Err(
                "RUSTPLC_WEB_USERS_JSON users require non-empty username, password, and display_name"
                    .to_string(),
            );
        }

        let mut seen = std::collections::HashSet::new();
        if configured
            .iter()
            .any(|user| !seen.insert(user.username.clone()))
        {
            return Err("RUSTPLC_WEB_USERS_JSON contains duplicate usernames".to_string());
        }

        let users = configured
            .into_iter()
            .map(|configured| UserCredential {
                user: AuthUser {
                    id: configured.username.clone(),
                    name: configured
                        .display_name
                        .unwrap_or_else(|| configured.username.clone()),
                    role: configured.role,
                },
                username: configured.username,
                password: configured.password,
            })
            .collect();

        Ok(Self::new(users, DEFAULT_SESSION_TTL))
    }

    fn new(users: Vec<UserCredential>, session_ttl: Duration) -> Self {
        Self {
            users: Arc::new(users),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_ttl,
        }
    }

    pub(crate) fn has_users(&self) -> bool {
        !self.users.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn disabled() -> Self {
        Self::new(Vec::new(), DEFAULT_SESSION_TTL)
    }

    #[cfg(test)]
    pub(crate) fn for_test_user(username: &str, password: &str, role: UserRole) -> Self {
        Self::new(
            vec![UserCredential {
                user: AuthUser {
                    id: username.to_string(),
                    name: username.to_string(),
                    role,
                },
                username: username.to_string(),
                password: password.to_string(),
            }],
            DEFAULT_SESSION_TTL,
        )
    }

    pub(crate) async fn login(&self, request: LoginRequest) -> Result<LoginResponse, AuthError> {
        let credential = self
            .users
            .iter()
            .find(|credential| {
                constant_time_eq(request.username.as_bytes(), credential.username.as_bytes())
            })
            .filter(|credential| {
                constant_time_eq(request.password.as_bytes(), credential.password.as_bytes())
            })
            .ok_or(AuthError::InvalidCredentials)?;

        let token = Uuid::new_v4().to_string();
        let expires_at_ms = now_ms().saturating_add(self.session_ttl.as_millis() as u64);
        self.sessions.write().await.insert(
            token.clone(),
            AuthSession {
                user: credential.user.clone(),
                expires_at_ms,
            },
        );

        Ok(LoginResponse {
            token,
            expires_at_ms,
            user: credential.user.clone(),
        })
    }

    pub(crate) async fn authenticate(&self, token: &str) -> Result<AuthUser, AuthError> {
        let now = now_ms();
        let mut sessions = self.sessions.write().await;
        let Some(session) = sessions.get(token) else {
            return Err(AuthError::InvalidSession);
        };
        if session.expires_at_ms <= now {
            sessions.remove(token);
            return Err(AuthError::ExpiredSession);
        }
        Ok(session.user.clone())
    }

    pub(crate) async fn logout(&self, token: &str) -> bool {
        self.sessions.write().await.remove(token).is_some()
    }
}

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
}

pub(crate) fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty())
}

async fn login(
    State(state): State<Arc<AppState>>,
    Json(request): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, (StatusCode, Json<Value>)> {
    if !state.auth.has_users() {
        return Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "interactive authentication is not configured",
        ));
    }
    state
        .auth
        .login(request)
        .await
        .map(Json)
        .map_err(map_auth_error)
}

async fn logout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let token = bearer_token(&headers)
        .ok_or_else(|| api_error(StatusCode::UNAUTHORIZED, "authentication required"))?;
    if !state.auth.logout(token).await {
        return Err(api_error(StatusCode::UNAUTHORIZED, "session is not valid"));
    }
    Ok(Json(json!({ "logged_out": true })))
}

async fn me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<AuthUser>, (StatusCode, Json<Value>)> {
    let token = bearer_token(&headers)
        .ok_or_else(|| api_error(StatusCode::UNAUTHORIZED, "authentication required"))?;
    state
        .auth
        .authenticate(token)
        .await
        .map(Json)
        .map_err(map_auth_error)
}

fn demo_users() -> Vec<ConfiguredUser> {
    [
        ("engineer", "Compiler Engineer", UserRole::Engineer),
        (
            "electrical",
            "Electrical Engineer",
            UserRole::ElectricalEngineer,
        ),
        (
            "commissioning",
            "Commissioning Engineer",
            UserRole::CommissioningEngineer,
        ),
        ("safety", "Safety Reviewer", UserRole::SafetyReviewer),
        ("release", "Release Approver", UserRole::ReleaseApprover),
        ("admin", "Administrator", UserRole::Admin),
    ]
    .into_iter()
    .map(|(username, display_name, role)| ConfiguredUser {
        username: username.to_string(),
        password: "password".to_string(),
        display_name: Some(display_name.to_string()),
        role,
    })
    .collect()
}

fn map_auth_error(error: AuthError) -> (StatusCode, Json<Value>) {
    let message = match error {
        AuthError::InvalidCredentials => "invalid username or password",
        AuthError::InvalidSession => "session is not valid",
        AuthError::ExpiredSession => "session has expired",
    };
    api_error(StatusCode::UNAUTHORIZED, message)
}

fn api_error(status: StatusCode, message: &str) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({ "error": message, "message": message })),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthError {
    InvalidCredentials,
    InvalidSession,
    ExpiredSession,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
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

#[cfg(test)]
mod tests {
    use super::*;

    fn service(role: UserRole, ttl: Duration) -> AuthService {
        AuthService::new(
            vec![UserCredential {
                user: AuthUser {
                    id: "alice".to_string(),
                    name: "Alice".to_string(),
                    role,
                },
                username: "alice".to_string(),
                password: "correct".to_string(),
            }],
            ttl,
        )
    }

    #[tokio::test]
    async fn login_creates_attributable_session() {
        let service = service(UserRole::SafetyReviewer, Duration::from_secs(60));
        let login = service
            .login(LoginRequest {
                username: "alice".to_string(),
                password: "correct".to_string(),
            })
            .await
            .expect("credentials should be accepted");

        assert_eq!(login.user.id, "alice");
        assert_eq!(login.user.role, UserRole::SafetyReviewer);
        assert_eq!(
            service
                .authenticate(&login.token)
                .await
                .expect("session should authenticate"),
            login.user
        );
    }

    #[tokio::test]
    async fn invalid_password_does_not_create_session() {
        let service = service(UserRole::Engineer, Duration::from_secs(60));
        let error = service
            .login(LoginRequest {
                username: "alice".to_string(),
                password: "wrong".to_string(),
            })
            .await
            .expect_err("wrong password must fail");
        assert_eq!(error, AuthError::InvalidCredentials);
    }

    #[tokio::test]
    async fn logout_revokes_session() {
        let service = service(UserRole::Engineer, Duration::from_secs(60));
        let login = service
            .login(LoginRequest {
                username: "alice".to_string(),
                password: "correct".to_string(),
            })
            .await
            .expect("credentials should be accepted");

        assert!(service.logout(&login.token).await);
        assert_eq!(
            service.authenticate(&login.token).await,
            Err(AuthError::InvalidSession)
        );
    }

    #[test]
    fn roles_only_sign_owned_hold_types() {
        assert!(!UserRole::Engineer.can_sign("wiring_review"));
        assert!(!UserRole::Engineer.can_sign("release_approval"));
        assert!(UserRole::ElectricalEngineer.can_sign("wiring_review"));
        assert!(UserRole::CommissioningEngineer.can_sign("hil_review"));
        assert!(UserRole::SafetyReviewer.can_sign("safety_review"));
        assert!(UserRole::ReleaseApprover.can_sign("release_approval"));
        assert!(UserRole::Admin.can_sign("release_approval"));
    }
}
