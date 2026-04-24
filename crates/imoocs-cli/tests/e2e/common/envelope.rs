use serde_json::Value;

/// stdout を成功 envelope `{success: true, data: <T>}` としてパースし、
/// `data` の中身を返す。バイト列が JSON でない / `success != true` のときは
/// 元の bytes を含めて panic する。
pub fn assert_success_envelope(stdout: &[u8]) -> Value {
    let envelope: Value = serde_json::from_slice(stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON: {e}\n--- stdout ---\n{}",
            String::from_utf8_lossy(stdout)
        )
    });
    let success = envelope
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("envelope has no boolean `success` key:\n{envelope:#}"));
    assert!(success, "expected success envelope, got failure:\n{envelope:#}");
    envelope
        .get("data")
        .cloned()
        .unwrap_or_else(|| panic!("success envelope is missing `data`:\n{envelope:#}"))
}

#[derive(Debug, Clone)]
pub struct FailureView {
    pub code: String,
    pub message: String,
    pub hint: Option<String>,
    pub details: Option<Value>,
}

/// stdout を失敗 envelope としてパースし、`error` の view を返す。構造が違う
/// 場合は panic。
pub fn assert_failure_envelope(stdout: &[u8]) -> FailureView {
    let envelope: Value = serde_json::from_slice(stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON: {e}\n--- stdout ---\n{}",
            String::from_utf8_lossy(stdout)
        )
    });
    let success = envelope
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("envelope has no boolean `success` key:\n{envelope:#}"));
    assert!(!success, "expected failure envelope, got success:\n{envelope:#}");
    let error = envelope
        .get("error")
        .unwrap_or_else(|| panic!("failure envelope is missing `error`:\n{envelope:#}"));
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("failure envelope has no `error.code`:\n{envelope:#}"))
        .to_string();
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("failure envelope has no `error.message`:\n{envelope:#}"))
        .to_string();
    let hint = error.get("hint").and_then(Value::as_str).map(String::from);
    let details = error.get("details").cloned();
    FailureView {
        code,
        message,
        hint,
        details,
    }
}
