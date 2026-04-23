//! 安定 JSON envelope (詳細は plan §CLI Design Principles #4)。
//!
//! - 成功時: `{ "success": true, "data": <T> }`
//! - 失敗時: `{ "success": false, "error": { "code", "message", "hint"?, "details"? } }`
//!
//! deserialize では未知の key を黙って無視する (forward compatibility)。
//! 既存の key は major version bump 無しで削除しない。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ImoocsError;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Envelope<T>
where
    T: Serialize + JsonSchema,
{
    Success { success: SuccessFlag, data: T },
    Failure { success: FailureFlag, error: ErrorDetail },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(from = "bool", into = "bool")]
pub struct SuccessFlag;

impl From<SuccessFlag> for bool {
    fn from(_: SuccessFlag) -> bool {
        true
    }
}

impl From<bool> for SuccessFlag {
    fn from(_: bool) -> Self {
        SuccessFlag
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(from = "bool", into = "bool")]
pub struct FailureFlag;

impl From<FailureFlag> for bool {
    fn from(_: FailureFlag) -> bool {
        false
    }
}

impl From<bool> for FailureFlag {
    fn from(_: bool) -> Self {
        FailureFlag
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl<T: Serialize + JsonSchema> Envelope<T> {
    pub fn success(data: T) -> Self {
        Self::Success {
            success: SuccessFlag,
            data,
        }
    }

    pub fn failure(error: ErrorDetail) -> Envelope<T> {
        Self::Failure {
            success: FailureFlag,
            error,
        }
    }
}

impl ErrorDetail {
    pub fn from_error(err: &ImoocsError) -> Self {
        Self {
            code: err.error_code().to_string(),
            message: err.to_string(),
            hint: err.hint().map(String::from),
            details: None,
        }
    }
}
