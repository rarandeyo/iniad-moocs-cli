use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

/// confirm モード (stage→push 2-step) に切り替える config.toml の中身。
/// confirm-stage の Walking Skeleton と章 4 の assignment_confirm で使う。
pub const CONFIG_CONFIRM: &str = "[assignment]\nconfirm = \"confirm\"\n";

/// 実行ごとに完全ユニークな marker (例: `e2e-1714030000000000000-<uuid>`)。
/// destructive テストで本当に MOOCs まで往復したことを過去実行や別 marker と
/// 衝突せずに証明するために使う。
pub fn unique_marker() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("e2e-{nanos}-{}", Uuid::new_v4())
}
