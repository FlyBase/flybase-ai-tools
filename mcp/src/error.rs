use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
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
    pub fn db(tool: &'static str, e: impl std::fmt::Display) -> Self {
        let text = e.to_string();
        tracing::error!(tool, error = %text, "database error");
        if text.contains("statement timeout") {
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
    #[test]
    fn timeout_maps_to_limit_exceeded_without_sql() {
        let e = ToolError::db(
            "search",
            "ERROR: canceling statement due to statement timeout: select * from secret",
        );
        assert_eq!(e.code(), "limit_exceeded");
        assert!(!e.to_string().contains("select"));
    }
}
