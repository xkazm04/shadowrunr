//! The one error taxonomy: at most ten codes, one envelope (`{"error": {code, message,
//! detail?}}`), used by `POST /rpc` replies and by error frames on `/events`.
//!
//! Older code names map onto the taxonomy with [`ErrorCode::from_legacy`]; the full table is in
//! docs/spec/contract.md § Error taxonomy.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Every error code the daemon may send. Clients branch on these and nothing else; the
/// `message` is for people and `detail` carries structured context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The request was malformed or its parameters are invalid (including a refused origin,
    /// a wrong content type, an unparseable body or a bad cursor).
    InvalidParams,
    /// The named run, tournament, report, character or rule does not exist.
    NotFound,
    /// The RPC method does not exist.
    MethodNotFound,
    /// A pre-flight check is red, so the action is refused.
    PreflightRed,
    /// A capacity limit is reached (live runs in flight, a slow consumer cut off); retry later.
    Busy,
    /// The LLM engine or a simulation failed (including a panicking sim).
    Engine,
    /// Reading or writing a file failed, or a stored file is malformed.
    Io,
    /// A dependency the call needs is not available (the daemon is shutting down, a source
    /// went away).
    Unavailable,
    /// The request conflicts with the current state (the run id is already running).
    Conflict,
    /// A daemon bug: the request was fine but the daemon could not answer it.
    Internal,
}

impl ErrorCode {
    /// Every code, in declaration order.
    pub const ALL: [ErrorCode; 10] = [
        ErrorCode::InvalidParams,
        ErrorCode::NotFound,
        ErrorCode::MethodNotFound,
        ErrorCode::PreflightRed,
        ErrorCode::Busy,
        ErrorCode::Engine,
        ErrorCode::Io,
        ErrorCode::Unavailable,
        ErrorCode::Conflict,
        ErrorCode::Internal,
    ];

    /// The wire name, as serde writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::InvalidParams => "invalid_params",
            ErrorCode::NotFound => "not_found",
            ErrorCode::MethodNotFound => "method_not_found",
            ErrorCode::PreflightRed => "preflight_red",
            ErrorCode::Busy => "busy",
            ErrorCode::Engine => "engine",
            ErrorCode::Io => "io",
            ErrorCode::Unavailable => "unavailable",
            ErrorCode::Conflict => "conflict",
            ErrorCode::Internal => "internal",
        }
    }

    /// Map a code name used before the taxonomy (or by another crate) onto it. A name that
    /// is already a taxonomy code maps to itself; an unknown name is `internal`.
    ///
    /// | legacy | code |
    /// |---|---|
    /// | `synthesis`, `sim_failed`, `engine` | `engine` |
    /// | `parse_error`, `forbidden_origin`, `unsupported_media_type`, `invalid_input` | `invalid_params` |
    /// | anything else unknown | `internal` |
    pub fn from_legacy(name: &str) -> ErrorCode {
        if let Some(c) = ErrorCode::ALL.iter().find(|c| c.as_str() == name) {
            return *c;
        }
        match name {
            "synthesis" | "sim_failed" => ErrorCode::Engine,
            "parse_error" | "forbidden_origin" | "unsupported_media_type" | "invalid_input" => {
                ErrorCode::InvalidParams
            }
            _ => ErrorCode::Internal,
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The body of every error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
    /// Structured context, e.g. `{"reason": "forbidden_origin"}` or
    /// `{"resume_since": 41}`. Omitted when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

/// `{"error": {...}}`: an RPC failure reply, or an error frame on `/events`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_codes_with_serde_names() {
        assert_eq!(ErrorCode::ALL.len(), 10);
        for c in ErrorCode::ALL {
            assert_eq!(serde_json::to_value(c).unwrap(), c.as_str());
            assert_eq!(ErrorCode::from_legacy(c.as_str()), c);
        }
    }

    #[test]
    fn legacy_codes_map_onto_the_taxonomy() {
        assert_eq!(ErrorCode::from_legacy("synthesis"), ErrorCode::Engine);
        assert_eq!(ErrorCode::from_legacy("sim_failed"), ErrorCode::Engine);
        assert_eq!(
            ErrorCode::from_legacy("forbidden_origin"),
            ErrorCode::InvalidParams
        );
        assert_eq!(ErrorCode::from_legacy("whatever"), ErrorCode::Internal);
    }

    #[test]
    fn detail_is_omitted_when_absent() {
        let e = ErrorEnvelope {
            error: ErrorBody {
                code: ErrorCode::NotFound,
                message: "run x".into(),
                detail: None,
            },
        };
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"error":{"code":"not_found","message":"run x"}}"#
        );
    }
}
