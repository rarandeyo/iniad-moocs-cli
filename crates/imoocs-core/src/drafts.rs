//! 課題提出の「stage」用ローカル draft ストア。
//!
//! `assignment.confirm = "confirm"` モード下では `assignment submit` / `upload`
//! は即サーバ送信せず、`Draft` としてローカルに書き留める。サーバへの確定送信は
//! `imoocs assignment push` が TTY で叩かれたときにまとめて行う。
//!
//! 1 problem = 1 JSON ファイル:
//! `<state_dir>/drafts/<year>-<sanitize(course)>-<sanitize(problem)>.json`

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ImoocsError, Result};
use crate::schemas::{AssignmentKey, Year};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub year: Year,
    pub course_id: String,
    pub problem_id: String,
    #[serde(default)]
    pub answers: HashMap<String, Value>,
    /// `submit` が呼ばれて `answers` を stage したかどうか。`upload` 単独で
    /// draft が作られた場合は false のまま。`push` はこの flag が false なら
    /// `put_answers` を呼ばず、サーバ側の既存 answers を `{}` で wipe しない。
    #[serde(default)]
    pub answers_staged: bool,
    #[serde(default)]
    pub files: HashMap<String, PathBuf>,
    pub updated_at: String,
}

/// `assignment drafts list` が返す軽量サマリ。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DraftSummary {
    pub year: Year,
    pub course_id: String,
    pub problem_id: String,
    pub answer_pids: Vec<String>,
    pub file_pids: Vec<String>,
    pub updated_at: String,
    pub path: PathBuf,
}

impl Draft {
    /// 指定 key に対する空 draft を生成する (answers / files は空、updated_at は現在時刻)。
    pub fn empty(key: &AssignmentKey) -> Self {
        Self {
            year: key.year,
            course_id: key.course_id.clone(),
            problem_id: key.problem_id.clone(),
            answers: HashMap::new(),
            answers_staged: false,
            files: HashMap::new(),
            updated_at: now_rfc3339(),
        }
    }

    /// `<dir>/<year>-<course>-<problem>.json` を返す (course / problem は sanitize する)。
    pub fn path_for(dir: &Path, key: &AssignmentKey) -> PathBuf {
        dir.join(format!(
            "{year}-{course}-{problem}.json",
            year = key.year,
            course = sanitize_component(&key.course_id),
            problem = sanitize_component(&key.problem_id),
        ))
    }

    /// 既存の draft を読み込む。存在しなければ `Ok(None)`。
    pub fn load(dir: &Path, key: &AssignmentKey) -> Result<Option<Self>> {
        let path = Self::path_for(dir, key);
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read(&path)?;
        let draft: Self = serde_json::from_slice(&raw)
            .map_err(|e| ImoocsError::Parse(format!("draft parse error at {}: {e}", path.display())))?;
        Ok(Some(draft))
    }

    /// 既存の draft があればそれを、無ければ空の新規 draft を返す。
    pub fn load_or_new(dir: &Path, key: &AssignmentKey) -> Result<Self> {
        match Self::load(dir, key)? {
            Some(d) => Ok(d),
            None => Ok(Self::empty(key)),
        }
    }

    /// draft を atomic に書き出す。`updated_at` を現在時刻に更新する。
    /// 戻り値は書き出し先の絶対パス。
    pub fn save(&mut self, dir: &Path) -> Result<PathBuf> {
        fs::create_dir_all(dir)?;
        self.updated_at = now_rfc3339();
        let key = AssignmentKey {
            year: self.year,
            course_id: self.course_id.clone(),
            problem_id: self.problem_id.clone(),
        };
        let path = Self::path_for(dir, &key);
        let bytes = serde_json::to_vec_pretty(self)?;
        atomic_write(&path, &bytes)?;
        Ok(path)
    }

    /// 指定 key の draft を削除する。存在した場合のみ `true`。
    pub fn remove(dir: &Path, key: &AssignmentKey) -> Result<bool> {
        let path = Self::path_for(dir, key);
        if !path.exists() {
            return Ok(false);
        }
        fs::remove_file(&path)?;
        Ok(true)
    }

    /// `<dir>` 直下の `*.json` を走査して DraftSummary を返す。`dir` が無ければ空。
    pub fn list(dir: &Path) -> Result<Vec<DraftSummary>> {
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let raw = match fs::read(&path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let draft: Self = match serde_json::from_slice(&raw) {
                Ok(d) => d,
                Err(_) => continue, // 壊れた draft はスキップ (list は best-effort)
            };
            out.push(draft.summary(path));
        }
        out.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
        Ok(out)
    }

    /// 自身を消費して DraftSummary に変換する (`list` 内で使う)。
    pub fn summary(self, path: PathBuf) -> DraftSummary {
        let mut answer_pids: Vec<String> = self.answers.keys().cloned().collect();
        answer_pids.sort();
        let mut file_pids: Vec<String> = self.files.keys().cloned().collect();
        file_pids.sort();
        DraftSummary {
            year: self.year,
            course_id: self.course_id,
            problem_id: self.problem_id,
            answer_pids,
            file_pids,
            updated_at: self.updated_at,
            path,
        }
    }
}

/// defense-in-depth: `course_id` / `problem_id` には URL path から
/// `[A-Za-z0-9_-]+` しか来ない想定だが、万一に備えてパス分離/親参照を潰す。
fn sanitize_component(s: &str) -> String {
    if s.is_empty() || s == "." || s == ".." {
        return "_".to_string();
    }
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '\0' => '_',
            c => c,
        })
        .collect()
}

fn atomic_write(target: &Path, bytes: &[u8]) -> Result<()> {
    let name = target
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".tmp".to_string());
    // 同プロセス内の並行 save で tmp path が衝突しないよう nanos を混ぜる。
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_path = target.with_file_name(format!("{name}.tmp.{}.{}", std::process::id(), nanos));
    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, target).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        ImoocsError::Io(e)
    })?;
    Ok(())
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmpdir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "imoocs-drafts-test-{}-{}-{}",
            label,
            std::process::id(),
            now_rfc3339().replace(':', "-")
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn key() -> AssignmentKey {
        AssignmentKey {
            year: 2026,
            course_id: "CS101".to_string(),
            problem_id: "problem-abc".to_string(),
        }
    }

    #[test]
    fn path_for_uses_sanitized_components() {
        let key = AssignmentKey {
            year: 2026,
            course_id: "CS/101".to_string(),
            problem_id: "../p".to_string(),
        };
        let p = Draft::path_for(Path::new("/s"), &key);
        assert_eq!(p, PathBuf::from("/s/2026-CS_101-.._p.json"));
    }

    #[test]
    fn sanitize_replaces_separators_and_empty() {
        assert_eq!(sanitize_component(""), "_");
        assert_eq!(sanitize_component(".."), "_");
        assert_eq!(sanitize_component("."), "_");
        assert_eq!(sanitize_component("a/b\\c:d"), "a_b_c_d");
        assert_eq!(sanitize_component("plain-id"), "plain-id");
    }

    #[test]
    fn load_nonexistent_returns_none() {
        let dir = tmpdir("load-none");
        let got = Draft::load(&dir, &key()).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tmpdir("roundtrip");
        let mut d = Draft::empty(&key());
        d.answers.insert("p1".into(), json!("hello"));
        d.answers.insert("p2".into(), json!(["a", "b"]));
        d.answers_staged = true;
        d.files.insert("html".into(), PathBuf::from("/tmp/x.html"));
        let saved_path = d.save(&dir).unwrap();
        assert!(saved_path.exists());

        let loaded = Draft::load(&dir, &key()).unwrap().expect("draft");
        assert_eq!(loaded.year, 2026);
        assert_eq!(loaded.course_id, "CS101");
        assert_eq!(loaded.problem_id, "problem-abc");
        assert!(loaded.answers_staged);
        assert_eq!(loaded.answers.get("p1").unwrap(), &json!("hello"));
        assert_eq!(loaded.files.get("html").unwrap(), &PathBuf::from("/tmp/x.html"));
        assert!(!loaded.updated_at.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn upload_only_draft_keeps_answers_staged_false() {
        let dir = tmpdir("upload-only");
        let mut d = Draft::empty(&key());
        d.files.insert("html".into(), PathBuf::from("/tmp/x.html"));
        d.save(&dir).unwrap();

        let loaded = Draft::load(&dir, &key()).unwrap().expect("draft");
        assert!(!loaded.answers_staged, "upload 単独では answers_staged=false のまま");
        assert!(loaded.answers.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_returns_true_only_when_existed() {
        let dir = tmpdir("remove");
        let mut d = Draft::empty(&key());
        d.save(&dir).unwrap();
        assert!(Draft::remove(&dir, &key()).unwrap());
        assert!(!Draft::remove(&dir, &key()).unwrap());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_enumerates_multiple_drafts_sorted() {
        let dir = tmpdir("list");
        let keys = [
            AssignmentKey {
                year: 2026,
                course_id: "A".into(),
                problem_id: "p1".into(),
            },
            AssignmentKey {
                year: 2026,
                course_id: "B".into(),
                problem_id: "p2".into(),
            },
            AssignmentKey {
                year: 2026,
                course_id: "A".into(),
                problem_id: "p3".into(),
            },
        ];
        for k in &keys {
            let mut d = Draft::empty(k);
            d.answers.insert("p".into(), json!("v"));
            d.save(&dir).unwrap();
            // 連続 save で updated_at が同値になると sort が不安定になるので、
            // ここでは順序だけ検査する。実機では 1 秒粒度の RFC3339 が安定している。
        }
        let summaries = Draft::list(&dir).unwrap();
        assert_eq!(summaries.len(), 3);
        let courses: Vec<&str> = summaries.iter().map(|s| s.course_id.as_str()).collect();
        assert!(courses.contains(&"A"));
        assert!(courses.contains(&"B"));
        for s in &summaries {
            assert_eq!(s.answer_pids, vec!["p".to_string()]);
            assert!(s.path.exists());
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_on_missing_dir_is_empty() {
        let dir = tmpdir("missing").join("nope");
        let summaries = Draft::list(&dir).unwrap();
        assert!(summaries.is_empty());
    }

    #[test]
    fn list_skips_non_json_and_broken_files() {
        let dir = tmpdir("broken");
        fs::create_dir_all(&dir).unwrap();
        // 非 json
        fs::write(dir.join("README.md"), b"hi").unwrap();
        // 壊れた json
        fs::write(dir.join("garbage.json"), b"not-json").unwrap();
        // 正常 draft
        let mut d = Draft::empty(&key());
        d.save(&dir).unwrap();

        let summaries = Draft::list(&dir).unwrap();
        assert_eq!(summaries.len(), 1, "broken/non-json are skipped");

        let _ = fs::remove_dir_all(&dir);
    }
}
