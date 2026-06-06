//! `api/drive.rs` の単体テスト。本体ファイル肥大化を避けるため #[path] で
//! 物理ファイルを分離 (本体の `mod tests;` 宣言から参照)。
//!
//! private な `parse_xhr_page` / `classify_xhr_error` / `fetch_all_pages` /
//! `fetch_drive_query_pages` / `atomic_write` / `sapisid_hash` /
//! `build_folder_name_query` / `extract_filename` / `extension_from` /
//! `validate_drive_id` 全てに依存するので、integration test (`tests/`) で
//! はなく同一 crate 内の sub module として置く。

use super::*;

#[test]
fn extract_filename_basic() {
    assert_eq!(
        extract_filename(r#"attachment; filename="ai-01.zip""#).as_deref(),
        Some("ai-01.zip")
    );
    assert_eq!(extract_filename("attachment").as_deref(), None);
}

#[test]
fn extension_from_prefers_filename_suffix() {
    assert_eq!(extension_from("ai-01.zip", None).as_deref(), Some("zip"));
    assert_eq!(extension_from("weird", Some("application/pdf")).as_deref(), Some("pdf"));
    assert_eq!(extension_from("noext", None), None);
}

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

const XHR_PAGE1_FIXTURE: &str = include_str!("../../tests/fixtures/drive_xhr_page1.json");
const XHR_PAGE2_FIXTURE: &str = include_str!("../../tests/fixtures/drive_xhr_page2_last.json");

#[test]
fn sapisid_hash_known_answer() {
    let got = sapisid_hash(1_000_000_000, "SAMPLE_SAPISID", "https://drive.google.com");
    assert_eq!(got, "1000000000_f8e785b009b005421a7e7e2a5a40c6db42a37ac9");
}

#[test]
fn build_folder_name_query_exact_escapes_literal() {
    let got = build_folder_name_query("Bob's \\ folder", true);
    assert_eq!(
        got,
        "title = 'Bob\\'s \\\\ folder' and mimeType = 'application/vnd.google-apps.folder'"
    );
}

#[test]
fn build_folder_name_query_partial_uses_contains() {
    let got = build_folder_name_query("[受講生]講義資料", false);
    assert_eq!(
        got,
        "title contains '[受講生]講義資料' and mimeType = 'application/vnd.google-apps.folder'"
    );
}

#[test]
fn parse_xhr_page1_returns_items_and_next_token() {
    let (items, next) = parse_xhr_page(XHR_PAGE1_FIXTURE).expect("page1 parse");
    assert_eq!(items.len(), 3);
    assert_eq!(next.as_deref(), Some("FIXTURE_TOKEN_PAGE_2"));
    assert_eq!(items[0].name, "AI-01");
    assert_eq!(items[0].kind, DriveKind::Folder);
    assert_eq!(items[0].mime, "application/vnd.google-apps.folder");
    assert_eq!(items[1].name, "handout.pdf");
    assert_eq!(items[1].kind, DriveKind::File);
    assert_eq!(items[1].mime, "application/pdf");
    assert_eq!(items[2].modified_at.as_deref(), Some("2026-04-03T12:00:00.000Z"));
}

#[test]
fn parse_xhr_page2_terminates_without_next_token() {
    let (items, next) = parse_xhr_page(XHR_PAGE2_FIXTURE).expect("page2 parse");
    assert_eq!(items.len(), 2);
    assert!(next.is_none(), "last page should have no nextPageToken");
    assert_eq!(items[0].name, "notes.txt");
    assert!(items[0].modified_at.is_none());
    assert_eq!(items[1].name, "sub-folder");
    assert_eq!(items[1].kind, DriveKind::Folder);
}

#[test]
fn parse_xhr_page_error_on_shape_change() {
    let bad = r#"{"items": ["not an object"], "nextPageToken": null}"#;
    let err = parse_xhr_page(bad).unwrap_err();
    match err {
        ImoocsError::Parse(m) => assert!(m.contains("Drive XHR endpoint may have changed"), "got {m:?}"),
        other => panic!("expected Parse error, got {other:?}"),
    }
}

#[test]
fn classify_xhr_error_unregistered_caller_maps_to_api() {
    let body = r#"{"error":{"code":403,"message":"Method doesn't allow unregistered callers (callers without established identity). Please use API Key or other form of API consumer identity to call this API."}}"#;
    let err = classify_xhr_error(StatusCode::FORBIDDEN, body, "test folder");
    match err {
        ImoocsError::Api(m) => {
            assert!(m.contains("rejected our API key"), "got {m:?}");
            assert!(m.contains("rotated upstream"), "should hint at regression, got {m:?}");
        }
        other => panic!("expected Api error (API-key regression), got {other:?}"),
    }
}

#[test]
fn classify_xhr_error_permission_denied_maps_to_auth() {
    let body = r#"{"error":{"code":403,"message":"The caller does not have permission"}}"#;
    let err = classify_xhr_error(StatusCode::FORBIDDEN, body, "test folder");
    assert!(
        matches!(err, ImoocsError::Auth { .. }),
        "expected Auth error, got {err:?}"
    );
}

#[test]
fn classify_xhr_error_invalid_query_maps_to_api() {
    let body = r#"{"error":{"code":400,"message":"Invalid Value"}}"#;
    let err = classify_xhr_error(StatusCode::BAD_REQUEST, body, "test search");
    match err {
        ImoocsError::Api(m) => assert!(m.contains("Query semantics may have changed"), "got {m:?}"),
        other => panic!("expected Api error, got {other:?}"),
    }
}

#[test]
fn classify_xhr_error_404_maps_to_not_found() {
    let body = r#"{"error":{"code":404,"message":"File not found"}}"#;
    let err = classify_xhr_error(StatusCode::NOT_FOUND, body, "test folder");
    assert!(matches!(err, ImoocsError::NotFound { .. }), "got {err:?}");
}

// Phase D-2: `fetch_all_pages` (= folder children query を渡す wrapper) は削除。
// list_drive_folder は agent-browser DOM scrape 経路に置換済み。
// page-token chaining と 403 handling のカバレッジは `fetch_drive_query_pages_uses_arbitrary_query`
// 含む下流の test で維持する (= 同じ `fetch_drive_query_pages` を search 経由で叩く)。

#[tokio::test]
async fn fetch_drive_query_pages_uses_arbitrary_query() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("GET", "/drive/v2beta/files")
        .match_query(mockito::Matcher::UrlEncoded(
            "q".into(),
            "title = '[受講生]講義資料' and mimeType = 'application/vnd.google-apps.folder'".into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json; charset=UTF-8")
        .with_body(
            r#"{"items":[
                {"id":"FOLDER_A","title":"[受講生]講義資料","mimeType":"application/vnd.google-apps.folder"},
                {"id":"FILE_B","title":"ignore.pdf","mimeType":"application/pdf"}
            ]}"#,
        )
        .expect(1)
        .create_async()
        .await;

    let endpoint = format!("{}/drive/v2beta/files", server.url());
    let client = reqwest::Client::new();
    let items = fetch_drive_query_pages(
        &client,
        &endpoint,
        "SAPISIDHASH fake",
        "title = '[受講生]講義資料' and mimeType = 'application/vnd.google-apps.folder'",
        "drive folder search",
    )
    .await
    .expect("fetch_drive_query_pages should succeed");

    assert_eq!(items.len(), 2);
    assert_eq!(items[0].kind, DriveKind::Folder);
    assert_eq!(items[1].kind, DriveKind::File);

    m.assert_async().await;
}

#[test]
fn atomic_write_replaces_existing_file() {
    let dir = std::env::temp_dir().join(format!("imoocs-atomic-write-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let target = dir.join("file.bin");
    atomic_write(&target, b"first").unwrap();
    atomic_write(&target, b"second").unwrap();
    let got = fs::read(&target).unwrap();
    assert_eq!(got, b"second");
    let remaining: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.file_name()))
        .collect();
    assert_eq!(remaining.len(), 1, "expected only target file, got {remaining:?}");
    let _ = fs::remove_dir_all(&dir);
}
