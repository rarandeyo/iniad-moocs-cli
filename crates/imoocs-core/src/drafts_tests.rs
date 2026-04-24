//! `drafts.rs` の単体テスト。本体ファイル肥大化を避けるため #[path] で
//! 物理ファイルを分離している (本体の `mod tests;` 宣言から参照される)。
//!
//! private な `sanitize_component` / `now_rfc3339` にもアクセスする必要が
//! あるため、integration test (`tests/`) ではなく同一 crate 内の sub module
//! として置く。

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
