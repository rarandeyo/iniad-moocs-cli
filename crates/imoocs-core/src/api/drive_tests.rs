//! `api/drive.rs` の単体テスト。本体ファイル肥大化を避けるため #[path] で
//! 物理ファイルを分離 (本体の `mod tests;` 宣言から参照)。
//!
//! Phase D-2 で reqwest XHR 経路を全削除し、agent-browser DOM scrape に置換した。
//! 旧 SAPISIDHASH / XHR JSON parse / HTTP error classification のテストは
//! 一緒に削除済み。残るは ID 形式バリデーションと MIME 推定の単体テスト。

use super::*;

#[test]
fn validate_drive_id_accepts_typical_ids() {
    validate_drive_id("FAKE_DRIVE_FILE_ID_FOR_TESTS_0001").unwrap();
    validate_drive_id("FAKE_DRIVE_FOLDER_ID_FOR_TESTS_0001").unwrap();
    validate_drive_id("abc").unwrap();
}

#[test]
fn validate_drive_id_rejects_path_traversal() {
    for bad in &[
        "",
        "../../etc/passwd",
        "..",
        "a/b",
        "foo.bar",
        "has space",
        "with#hash",
        "with?q",
        &"x".repeat(129),
    ] {
        assert!(
            validate_drive_id(bad).is_err(),
            "validate_drive_id should reject {bad:?}"
        );
    }
}

#[test]
fn validate_drive_id_or_root_accepts_root_aliases() {
    validate_drive_id_or_root("root").unwrap();
    validate_drive_id_or_root("my-drive").unwrap();
    validate_drive_id_or_root("FAKE_DRIVE_FOLDER_ID_FOR_TESTS_0001").unwrap();
}

#[test]
fn validate_drive_id_or_root_rejects_garbage() {
    assert!(validate_drive_id_or_root("../etc").is_err());
    assert!(validate_drive_id_or_root("has space").is_err());
}

#[test]
fn infer_mime_folder_overrides_tooltip() {
    // folder kind なら tooltip の中身に関わらず folder MIME
    assert_eq!(
        infer_mime_from_tooltip("anything PDF", DriveKind::Folder),
        "application/vnd.google-apps.folder"
    );
}

#[test]
fn infer_mime_from_tooltip_known_suffixes() {
    assert_eq!(
        infer_mime_from_tooltip("foo.pdf PDF", DriveKind::File),
        "application/pdf"
    );
    assert_eq!(
        infer_mime_from_tooltip("プレゼン Google スライド", DriveKind::File),
        "application/vnd.google-apps.presentation"
    );
    assert_eq!(
        infer_mime_from_tooltip("資料 Google ドキュメント", DriveKind::File),
        "application/vnd.google-apps.document"
    );
    assert_eq!(
        infer_mime_from_tooltip("集計 Google スプレッドシート", DriveKind::File),
        "application/vnd.google-apps.spreadsheet"
    );
    assert_eq!(
        infer_mime_from_tooltip("アンケート Google フォーム", DriveKind::File),
        "application/vnd.google-apps.form"
    );
}

#[test]
fn infer_mime_unknown_tooltip_falls_back_to_octet_stream() {
    assert_eq!(
        infer_mime_from_tooltip("notebook.ipynb Colab notebook", DriveKind::File),
        "application/octet-stream"
    );
    assert_eq!(infer_mime_from_tooltip("", DriveKind::File), "application/octet-stream");
}
