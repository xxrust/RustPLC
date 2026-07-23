use axum::http::HeaderValue;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub(crate) struct WebSecurityConfig {
    pub(crate) auth_token: Option<Arc<str>>,
    pub(crate) allowed_origins: Vec<HeaderValue>,
}

#[derive(Clone)]
pub(crate) enum RustPlcLauncher {
    Cargo,
    Binary(PathBuf),
}

pub(crate) struct WebConfig {
    pub(crate) bind_addr: SocketAddr,
    pub(crate) security: WebSecurityConfig,
    pub(crate) max_concurrent_runs: usize,
    pub(crate) run_timeout: Duration,
    pub(crate) rust_plc_launcher: RustPlcLauncher,
}

impl WebConfig {
    pub(crate) fn from_env() -> Result<Self, String> {
        let bind_addr = std::env::var("RUSTPLC_WEB_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:8080".to_string())
            .parse::<SocketAddr>()
            .map_err(|_| "RUSTPLC_WEB_ADDR must be an IP socket address".to_string())?;
        let allow_remote = env_flag("RUSTPLC_WEB_ALLOW_REMOTE");
        let auth_token = std::env::var("RUSTPLC_WEB_AUTH_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Arc::<str>::from);
        let origins_were_explicit = std::env::var_os("RUSTPLC_WEB_ALLOWED_ORIGINS").is_some();
        let allowed_origins = parse_allowed_origins(
            std::env::var("RUSTPLC_WEB_ALLOWED_ORIGINS")
                .unwrap_or_else(|_| default_loopback_origins(bind_addr).join(","))
                .as_str(),
        )?;

        validate_bind_security(
            bind_addr,
            allow_remote,
            auth_token.is_some(),
            origins_were_explicit,
            !allowed_origins.is_empty(),
        )?;

        let max_concurrent_runs = parse_bounded_env_usize(
            "RUSTPLC_WEB_MAX_CONCURRENT_RUNS",
            crate::DEFAULT_MAX_CONCURRENT_RUNS,
            1,
            32,
        )?;
        let timeout_secs = parse_bounded_env_u64(
            "RUSTPLC_WEB_RUN_TIMEOUT_SECS",
            crate::DEFAULT_RUN_TIMEOUT_SECS,
            1,
            3600,
        )?;
        let rust_plc_launcher = match std::env::var("RUSTPLC_WEB_RUST_PLC_BIN") {
            Ok(path) if !path.trim().is_empty() => {
                let path = PathBuf::from(path)
                    .canonicalize()
                    .map_err(|_| "RUSTPLC_WEB_RUST_PLC_BIN does not exist".to_string())?;
                if !path.is_file() {
                    return Err("RUSTPLC_WEB_RUST_PLC_BIN must reference a file".to_string());
                }
                RustPlcLauncher::Binary(path)
            }
            _ => RustPlcLauncher::Cargo,
        };

        Ok(Self {
            bind_addr,
            security: WebSecurityConfig {
                auth_token,
                allowed_origins,
            },
            max_concurrent_runs,
            run_timeout: Duration::from_secs(timeout_secs),
            rust_plc_launcher,
        })
    }
}

pub(crate) fn validate_bind_security(
    bind_addr: SocketAddr,
    allow_remote: bool,
    has_auth_token: bool,
    origins_were_explicit: bool,
    has_allowed_origins: bool,
) -> Result<(), String> {
    if bind_addr.ip().is_loopback() {
        return Ok(());
    }
    if !allow_remote {
        return Err(
            "non-loopback RUSTPLC_WEB_ADDR requires RUSTPLC_WEB_ALLOW_REMOTE=1".to_string(),
        );
    }
    if !has_auth_token {
        return Err("non-loopback binding requires a non-empty RUSTPLC_WEB_AUTH_TOKEN".to_string());
    }
    if !origins_were_explicit || !has_allowed_origins {
        return Err(
            "non-loopback binding requires explicit RUSTPLC_WEB_ALLOWED_ORIGINS".to_string(),
        );
    }
    Ok(())
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn default_loopback_origins(bind_addr: SocketAddr) -> Vec<String> {
    vec![
        format!("http://127.0.0.1:{}", bind_addr.port()),
        format!("http://localhost:{}", bind_addr.port()),
        format!("http://[::1]:{}", bind_addr.port()),
    ]
}

pub(crate) fn parse_allowed_origins(raw: &str) -> Result<Vec<HeaderValue>, String> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            if value == "*" {
                return Err("wildcard CORS origins are not allowed".to_string());
            }
            HeaderValue::from_str(value).map_err(|_| format!("invalid CORS origin `{value}`"))
        })
        .collect()
}

fn parse_bounded_env_usize(
    name: &str,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize, String> {
    let Some(raw) = std::env::var(name).ok() else {
        return Ok(default);
    };
    let value = raw
        .parse::<usize>()
        .map_err(|_| format!("{name} must be an integer"))?;
    if !(min..=max).contains(&value) {
        return Err(format!("{name} must be between {min} and {max}"));
    }
    Ok(value)
}

fn parse_bounded_env_u64(name: &str, default: u64, min: u64, max: u64) -> Result<u64, String> {
    let Some(raw) = std::env::var(name).ok() else {
        return Ok(default);
    };
    let value = raw
        .parse::<u64>()
        .map_err(|_| format!("{name} must be an integer"))?;
    if !(min..=max).contains(&value) {
        return Err(format!("{name} must be between {min} and {max}"));
    }
    Ok(value)
}
