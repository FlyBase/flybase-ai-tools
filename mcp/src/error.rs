use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::json;
use tokio_postgres::error::SqlState;

#[derive(Debug, Clone, thiserror::Error)]
pub enum ToolError {
    #[error("{tool}: invalid {arg}: {message}")]
    InvalidInput {
        tool: &'static str,
        arg: &'static str,
        message: String,
    },
    #[error("{tool}: {message}")]
    NotFound { tool: &'static str, message: String },
    #[error("{tool}: {message}")]
    BackendUnavailable { tool: &'static str, message: String },
    #[error("{tool}: {message}")]
    LimitExceeded { tool: &'static str, message: String },
}

impl ToolError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput { .. } => "invalid_input",
            Self::NotFound { .. } => "not_found",
            Self::BackendUnavailable { .. } => "backend_unavailable",
            Self::LimitExceeded { .. } => "limit_exceeded",
        }
    }

    pub fn into_result(self) -> CallToolResult {
        let msg = self.to_string();
        let mut r =
            CallToolResult::structured_error(json!({ "error": self.code(), "message": msg }));
        r.content = vec![ContentBlock::text(msg)];
        r
    }

    /// Wrap a database error without leaking SQL text to the client.
    /// The full cause (SQLSTATE and server message) is logged for operators.
    pub fn db(tool: &'static str, e: &tokio_postgres::Error) -> Self {
        let canceled = e.code() == Some(&SqlState::QUERY_CANCELED);
        // A statement timeout is an expected outcome of an over-broad request, not a server fault.
        match (e.as_db_error(), canceled) {
            (Some(d), true) => tracing::warn!(
                tool,
                sqlstate = d.code().code(),
                error = d.message(),
                "database query canceled"
            ),
            (Some(d), false) => tracing::error!(
                tool,
                sqlstate = d.code().code(),
                error = d.message(),
                "database error"
            ),
            (None, _) => tracing::error!(tool, error = ?e, "database error"),
        }
        if canceled {
            Self::LimitExceeded {
                tool,
                message: "query timed out; narrow the request".into(),
            }
        } else {
            Self::BackendUnavailable {
                tool,
                message: "database error; try again shortly".into(),
            }
        }
    }
}

pub type ToolResult<T> = Result<T, ToolError>;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_input_names_tool_and_arg() {
        let e = ToolError::InvalidInput {
            tool: "get_sequence",
            arg: "region",
            message: "end before start".into(),
        };
        let r = e.into_result();
        assert_eq!(r.is_error, Some(true));
        let s = r.structured_content.unwrap();
        assert_eq!(s["error"], "invalid_input");
        assert!(
            s["message"]
                .as_str()
                .unwrap()
                .starts_with("get_sequence: invalid region")
        );
    }
}
