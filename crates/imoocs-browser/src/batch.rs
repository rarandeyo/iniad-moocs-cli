use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

/// `agent-browser wait --load <kind>` の引数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadKind {
    Load,
    DomContentLoaded,
    NetworkIdle,
}

impl LoadKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Load => "load",
            Self::DomContentLoaded => "domcontentloaded",
            Self::NetworkIdle => "networkidle",
        }
    }
}

/// 要素の指し方。a11y snapshot で得た `@eN` 参照、または CSS セレクタ。
#[derive(Debug, Clone)]
pub enum Target {
    Ref(String),
    Css(String),
}

impl Target {
    pub fn as_token(&self) -> String {
        match self {
            Self::Ref(r) => {
                if r.starts_with('@') {
                    r.clone()
                } else {
                    format!("@{r}")
                }
            }
            Self::Css(s) => s.clone(),
        }
    }
}

/// 1 つの `batch` 中の単一コマンド (`Vec<String>` と等価)。
pub type BatchCommand = Vec<String>;

/// `batch --json` 全体のレスポンス。
pub type BatchResponse = Vec<BatchOutcome>;

/// `batch --json` の per-command 結果。実機検証で確認した shape。
#[derive(Debug, Clone, Deserialize)]
pub struct BatchOutcome {
    pub command: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub result: Option<Value>,
    pub success: bool,
}

/// `batch --json` 入力の JSON 配列を組み立てるビルダ。
///
/// 設計方針:
/// - 1 batch = 1 spawn で daemon ラウンドトリップを最小化
/// - メソッドチェーンで宣言的に組める
/// - 個別 command の Vec<String> をそのまま JSON 化する
#[derive(Debug, Default, Clone)]
pub struct BatchBuilder {
    commands: Vec<BatchCommand>,
}

impl BatchBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 任意の生コマンドを追加する低レベル API。
    pub fn push<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.commands.push(args.into_iter().map(Into::into).collect());
        self
    }

    pub fn open(&mut self, url: impl Into<String>) -> &mut Self {
        self.push(["open".to_string(), url.into()])
    }

    pub fn navigate(&mut self, url: impl Into<String>) -> &mut Self {
        self.open(url)
    }

    pub fn wait_load(&mut self, kind: LoadKind) -> &mut Self {
        self.push(["wait", "--load", kind.as_str()])
    }

    pub fn wait_url(&mut self, glob: impl Into<String>, timeout_ms: u32) -> &mut Self {
        self.push([
            "wait".to_string(),
            "--url".to_string(),
            glob.into(),
            "--timeout".to_string(),
            timeout_ms.to_string(),
        ])
    }

    pub fn wait_fn(&mut self, expr: impl Into<String>, timeout_ms: u32) -> &mut Self {
        self.push([
            "wait".to_string(),
            "--fn".to_string(),
            expr.into(),
            "--timeout".to_string(),
            timeout_ms.to_string(),
        ])
    }

    pub fn wait_text(&mut self, text: impl Into<String>, timeout_ms: u32) -> &mut Self {
        self.push([
            "wait".to_string(),
            "--text".to_string(),
            text.into(),
            "--timeout".to_string(),
            timeout_ms.to_string(),
        ])
    }

    pub fn wait_ms(&mut self, ms: u32) -> &mut Self {
        self.push(["wait".to_string(), ms.to_string()])
    }

    pub fn snapshot_interactive(&mut self) -> &mut Self {
        self.push(["snapshot", "-i"])
    }

    pub fn snapshot_scoped(&mut self, css: impl Into<String>) -> &mut Self {
        self.push(["snapshot".to_string(), "-i".to_string(), "-s".to_string(), css.into()])
    }

    pub fn click(&mut self, target: &Target) -> &mut Self {
        self.push(["click".to_string(), target.as_token()])
    }

    pub fn fill(&mut self, target: &Target, value: impl Into<String>) -> &mut Self {
        self.push(["fill".to_string(), target.as_token(), value.into()])
    }

    pub fn upload<P: AsRef<Path>>(&mut self, target: &Target, files: &[P]) -> &mut Self {
        let mut args: Vec<String> = vec!["upload".into(), target.as_token()];
        for f in files {
            args.push(f.as_ref().display().to_string());
        }
        self.push(args)
    }

    pub fn set_viewport(&mut self, width: u32, height: u32) -> &mut Self {
        self.push([
            "set".to_string(),
            "viewport".to_string(),
            width.to_string(),
            height.to_string(),
        ])
    }

    pub fn pdf(&mut self, path: &Path) -> &mut Self {
        self.push(["pdf".to_string(), path.display().to_string()])
    }

    pub fn eval(&mut self, script: impl Into<String>) -> &mut Self {
        self.push(["eval".to_string(), script.into()])
    }

    pub fn get_url(&mut self) -> &mut Self {
        self.push(["get", "url"])
    }

    pub fn get_html(&mut self, target: &Target) -> &mut Self {
        self.push(["get".to_string(), "html".to_string(), target.as_token()])
    }

    pub fn get_text(&mut self, target: &Target) -> &mut Self {
        self.push(["get".to_string(), "text".to_string(), target.as_token()])
    }

    pub fn state_save(&mut self, path: &Path) -> &mut Self {
        self.push(["state".to_string(), "save".to_string(), path.display().to_string()])
    }

    pub fn state_load(&mut self, path: &Path) -> &mut Self {
        self.push(["state".to_string(), "load".to_string(), path.display().to_string()])
    }

    /// 組み立てた command 配列を `Vec<Vec<String>>` として取り出す。
    pub fn build(&self) -> &[BatchCommand] {
        &self.commands
    }

    /// JSON 文字列 (`agent-browser batch --json` の stdin に渡す形式) にする。
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(&self.commands)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn target_ref_normalization() {
        assert_eq!(Target::Ref("e2".into()).as_token(), "@e2");
        assert_eq!(Target::Ref("@e2".into()).as_token(), "@e2");
        assert_eq!(Target::Css("input#id".into()).as_token(), "input#id");
    }

    #[test]
    fn batch_builder_open_wait_pdf() {
        let mut b = BatchBuilder::new();
        b.set_viewport(1280, 720)
            .open("https://example.com")
            .wait_load(LoadKind::NetworkIdle)
            .pdf(&PathBuf::from("/tmp/out.pdf"));
        let json = b.to_json().unwrap();
        assert!(json.contains("\"set\""));
        assert!(json.contains("\"viewport\""));
        assert!(json.contains("\"1280\""));
        assert!(json.contains("\"open\""));
        assert!(json.contains("\"https://example.com\""));
        assert!(json.contains("\"wait\""));
        assert!(json.contains("\"networkidle\""));
        assert!(json.contains("\"pdf\""));
        assert!(json.contains("/tmp/out.pdf"));
    }

    #[test]
    fn batch_builder_fill_click() {
        let mut b = BatchBuilder::new();
        b.fill(&Target::Css("#username".into()), "alice")
            .fill(&Target::Css("#password".into()), "p@ss")
            .click(&Target::Css("#kc-login".into()));
        let cmds = b.build();
        assert_eq!(cmds.len(), 3);
        assert_eq!(cmds[0], vec!["fill", "#username", "alice"]);
        assert_eq!(cmds[1], vec!["fill", "#password", "p@ss"]);
        assert_eq!(cmds[2], vec!["click", "#kc-login"]);
    }

    #[test]
    fn batch_outcome_deserialization() {
        let raw = r#"[
            {"command":["open","https://example.com"],"error":null,"result":{"url":"https://example.com"},"success":true},
            {"command":["snapshot"],"error":"timeout","result":null,"success":false}
        ]"#;
        let outcomes: BatchResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(outcomes.len(), 2);
        assert!(outcomes[0].success);
        assert!(!outcomes[1].success);
        assert_eq!(outcomes[1].error.as_deref(), Some("timeout"));
    }
}
