//! Bearer authentication for the job routes (design 3.2). The token comes from `BLAST_JOBS_TOKEN`; an
//! empty or unset token refuses to start unless `BLAST_JOBS_ALLOW_NO_AUTH=1` (local runs). The token is
//! compared in constant time and never logged.
use crate::http::AppState;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::fmt;
use std::sync::Arc;
use subtle::ConstantTimeEq;

pub const TOKEN_ENV: &str = "BLAST_JOBS_TOKEN";
pub const ALLOW_NO_AUTH_ENV: &str = "BLAST_JOBS_ALLOW_NO_AUTH";

#[derive(Clone)]
pub enum Auth {
    /// Every job route needs `Authorization: Bearer <token>`.
    Token(String),
    /// No authentication (only with `BLAST_JOBS_ALLOW_NO_AUTH=1`).
    Disabled,
}

/// Never prints the token.
impl fmt::Debug for Auth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Auth::Token(_) => f.write_str("Auth::Token(<redacted>)"),
            Auth::Disabled => f.write_str("Auth::Disabled"),
        }
    }
}

impl Auth {
    /// Reads `BLAST_JOBS_TOKEN` and `BLAST_JOBS_ALLOW_NO_AUTH`.
    pub fn from_env() -> anyhow::Result<Auth> {
        Self::from_values(
            std::env::var(TOKEN_ENV).ok(),
            std::env::var(ALLOW_NO_AUTH_ENV).ok(),
        )
    }

    /// `from_env` without the environment: a non-empty token wins; otherwise `allow_no_auth` must be
    /// exactly `"1"`.
    pub fn from_values(
        token: Option<String>,
        allow_no_auth: Option<String>,
    ) -> anyhow::Result<Auth> {
        match token {
            Some(t) if !t.trim().is_empty() => Ok(Auth::Token(t)),
            _ if allow_no_auth.as_deref() == Some("1") => Ok(Auth::Disabled),
            _ => anyhow::bail!(
                "{TOKEN_ENV} is empty or unset; set it, or set {ALLOW_NO_AUTH_ENV}=1 for a local run"
            ),
        }
    }

    /// Whether an `Authorization` header value carries the token. Constant time in the token bytes.
    pub fn accepts(&self, authorization: Option<&[u8]>) -> bool {
        match self {
            Auth::Disabled => true,
            Auth::Token(t) => authorization
                .and_then(|h| h.strip_prefix(b"Bearer "))
                .is_some_and(|given| bool::from(given.ct_eq(t.as_bytes()))),
        }
    }
}

/// Middleware for the job routes: `401 {"error":"unauthorized"}` unless the bearer token matches.
pub async fn require_bearer(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let given = req
        .headers()
        .get(header::AUTHORIZATION)
        .map(|v| v.as_bytes());
    if state.auth().accepts(given) {
        next.run(req).await
    } else {
        tracing::warn!(path = %req.uri().path(), "request with a missing or wrong bearer token");
        (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({"error": "unauthorized"})),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_required_unless_explicitly_disabled() {
        assert!(matches!(
            Auth::from_values(Some("s3cret".into()), None).unwrap(),
            Auth::Token(_)
        ));
        assert!(Auth::from_values(None, None).is_err());
        assert!(Auth::from_values(Some("  ".into()), None).is_err());
        assert!(Auth::from_values(None, Some("true".into())).is_err());
        assert!(matches!(
            Auth::from_values(None, Some("1".into())).unwrap(),
            Auth::Disabled
        ));
        assert!(matches!(
            Auth::from_values(Some("".into()), Some("1".into())).unwrap(),
            Auth::Disabled
        ));
    }

    #[test]
    fn accepts_only_the_exact_bearer_token() {
        let a = Auth::Token("s3cret".into());
        assert!(a.accepts(Some(b"Bearer s3cret")));
        assert!(!a.accepts(None));
        assert!(!a.accepts(Some(b"Bearer s3cre")));
        assert!(!a.accepts(Some(b"Bearer s3cretx")));
        assert!(!a.accepts(Some(b"bearer s3cret")));
        assert!(!a.accepts(Some(b"Basic s3cret")));
        assert!(!a.accepts(Some(b"s3cret")));
        assert!(Auth::Disabled.accepts(None));
    }

    #[test]
    fn debug_redacts_the_token() {
        let s = format!("{:?}", Auth::Token("s3cret".into()));
        assert!(!s.contains("s3cret"), "{s}");
    }
}
