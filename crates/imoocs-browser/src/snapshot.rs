use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

/// `snapshot -i` で得たアクセシビリティツリー + ref マップ。
#[derive(Debug, Clone, Deserialize)]
pub struct Snapshot {
    /// プレーンテキストの a11y ツリー (例: `- textbox "ユーザー名" [ref=e3]`)
    #[serde(default)]
    pub snapshot: String,
    /// `@eN → RefInfo` のマップ
    #[serde(default)]
    pub refs: HashMap<String, RefInfo>,
    /// `origin` URL (実機の snapshot envelope に含まれる)
    #[serde(default)]
    pub origin: Option<String>,
}

/// 個別要素のメタ情報。
#[derive(Debug, Clone, Deserialize)]
pub struct RefInfo {
    pub role: String,
    #[serde(default)]
    pub name: String,
    /// その他の属性 (level, checked, ...) を弱型で保持。
    #[serde(flatten)]
    pub extras: HashMap<String, Value>,
}

/// `snapshot` コマンドのオプション。
#[derive(Debug, Default, Clone)]
pub struct SnapshotOpts {
    pub interactive: bool,
    pub cursor: bool,
    pub compact: bool,
    pub depth: Option<u32>,
    pub scope: Option<String>,
}

impl Snapshot {
    /// `role` が一致する最初の要素の ref を返す。
    pub fn find_by_role(&self, role: &str) -> Option<(&str, &RefInfo)> {
        self.refs
            .iter()
            .find(|(_, info)| info.role == role)
            .map(|(k, v)| (k.as_str(), v))
    }

    /// `name` が部分一致する最初の要素の ref を返す。
    pub fn find_by_name_contains(&self, needle: &str) -> Option<(&str, &RefInfo)> {
        self.refs
            .iter()
            .find(|(_, info)| info.name.contains(needle))
            .map(|(k, v)| (k.as_str(), v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_deserialize_keycloak_form() {
        let raw = r#"{
            "snapshot": "- textbox \"ユーザー名\" [ref=e3]",
            "refs": {
                "e3": {"role": "textbox", "name": "ユーザー名"},
                "e4": {"role": "textbox", "name": "パスワード"},
                "e5": {"role": "button", "name": "LOG IN"}
            },
            "origin": "https://accounts.iniad.org/auth/realms/master/..."
        }"#;
        let snap: Snapshot = serde_json::from_str(raw).unwrap();
        assert_eq!(snap.refs.len(), 3);
        assert_eq!(snap.refs["e3"].role, "textbox");
        assert_eq!(snap.refs["e5"].name, "LOG IN");
    }

    #[test]
    fn find_by_name_contains() {
        let raw = r#"{
            "snapshot": "",
            "refs": {
                "e1": {"role": "button", "name": "続行"},
                "e2": {"role": "heading", "name": "本人確認"}
            }
        }"#;
        let snap: Snapshot = serde_json::from_str(raw).unwrap();
        let (key, info) = snap.find_by_name_contains("続行").unwrap();
        assert_eq!(key, "e1");
        assert_eq!(info.role, "button");
    }

    #[test]
    fn find_by_role_returns_first_match() {
        let raw = r#"{
            "snapshot": "",
            "refs": {
                "e1": {"role": "button", "name": "Cancel"},
                "e2": {"role": "textbox", "name": "search"}
            }
        }"#;
        let snap: Snapshot = serde_json::from_str(raw).unwrap();
        let result = snap.find_by_role("textbox");
        assert!(result.is_some());
        assert_eq!(result.unwrap().1.name, "search");
    }
}
