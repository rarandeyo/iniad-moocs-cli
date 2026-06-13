# Changelog

本プロジェクトの変更履歴です。形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に準拠し、
バージョニングは [Semantic Versioning](https://semver.org/lang/ja/) に従います。

## [Unreleased]

## [v0.3.1]

### Fixed

- `imoocs setup` のテキスト表示で、すでに設定済みの項目（`already set to …`）が
  `⚠` ではなく `✓` で表示されるように修正。

## [v0.3.0]

サーバへの書き込みと Google 系の取得を [agent-browser](https://github.com/vercel-labs/agent-browser)
(ヘッドレス Chrome) 経由に移行した。人間がブラウザを操作するのと同一の経路・同一の動作になる。

### Added

- `imoocs auth login` が agent-browser daemon 側にも MOOCs (Keycloak) + Google (SAML) の
  セッションを自動確立するようになった。
- Google セッションの自動回復: daemon が再起動してセッションが消えても、auth-vault の
  保存済み profile → SAML chain (本人確認ダイアログの自動クリック込み) で自動復活する。
- `drive fetch` がダウンロードファイルを元のファイル名 (Content-Disposition 由来) で
  保存するようになった。`<cache>/imoocs/drive/<fileId>/` 配下に 24h TTL でキャッシュ。
- Drive の E2E テスト (非認証 4 件 + 実 Drive round-trip 3 件) を追加。

### Changed

- **Breaking**: `assignment submit` / `upload` は `--url <課題ページURL>` が必須になった
  (positional の `COURSE_ID PROBLEM_ID` を削除)。URL 指定により course 一覧の走査が不要に
  なり、提出が大幅に高速化される。同一ページに複数課題がある場合は `--problem-id` で絞る。
- **Breaking**: `assignment push` は引数なしで全 draft を一括送信するようになった
  (`--url` で単一 draft に絞ることも可能)。確認プロンプトも 1 行サマリ + 詳細表示に刷新。
- **Breaking**: `submit` / `upload` / `push` / `drive list|search|fetch` / `slide fetch` /
  `auth login-google` は agent-browser のインストールが必要になった
  (`npm i -g agent-browser` または `cargo install agent-browser --locked`)。
- `drive list` / `search` は Drive Web UI の DOM から一覧を抽出する方式に変更
  (旧: 非公式 XHR endpoint + SAPISIDHASH 認証。endpoint の仕様変更で動作しなくなっていた)。
- `slide fetch` はスライドを 1 枚ずつ Chrome で描画して screenshot → PDF 合成する方式に変更
  (旧: 埋め込み SVG 抽出。色付き背景や日本語フォントの描画が不安定だった)。
  Web フォント・画像のロード完了を待つため、描画品質がブラウザ表示と同一になる。

### Removed

- 旧 Drive XHR 経路 (SAPISIDHASH / clients6.googleapis.com) と旧 SVG 抽出経路を削除。
  依存から `svg2pdf` / `pdf-writer` / `unicode_escape` / `base64` / `mime_guess` を除去。

### Internal

- workspace を `imoocs-types` / `imoocs-browser` / `imoocs-core` / `imoocs-cli` の 4 crate 構成に再編。
- destructive E2E を新 CLI 形式 (`--url` 必須) に追従、`IMOOCS_E2E_PAGE_URL` env を追加。

## [v0.2.0]

### Added
- `imoocs reset` サブコマンド: `auth` / `config` / `cache` / `drafts` / `all` の領域別に状態を初期化。`--dry-run` / `--yes` 対応。
- `course-drive-folders.json` に `unresolvedReason` フィールド (`deferred` / `not-offered` / `pending-folder` / `needs-user-input`) を追加し、再走時の挙動を分岐可能にした。

### Changed
- **Breaking**: `course-drive-folders.json` の `driveFolderId: String` を `driveFolders: []` (配列) に変更。1 コース複数フォルダ / 複数コース 1 フォルダ共有を表現できるようになった。
- `auth logout` は `config.toml` を残すようになった (削除されるのは keyring 認証情報と cookies のみ)。
- npm publish を OIDC trusted publishing に移行。配布パッケージの `engines.node` を `>=22` に引き上げ。

### Removed
- **Breaking**: `auth logout --keep-config` フラグを削除。代わりに `imoocs reset --scope auth` を使用。
- **Breaking**: `imoocs doctor` から agent skill 検出 (`DoctorReport.skills`) を削除。

### Internal
- Release workflow に CI gate (fmt / clippy / test) を追加。
- `imoocs-drive-setup` skill の名前正規化規範 (NFKC / ローマ数字 / 中黒 / 旧新名併記) を整備。
- README の agent skill インストール手順を刷新。

## [v0.1.0]

初回安定リリース。詳細は GitHub Releases を参照。

[Unreleased]: https://github.com/rarandeyo/iniad-moocs-cli/compare/v0.3.0...HEAD
[v0.3.0]: https://github.com/rarandeyo/iniad-moocs-cli/compare/v0.2.0...v0.3.0
[v0.2.0]: https://github.com/rarandeyo/iniad-moocs-cli/compare/v0.1.0...v0.2.0
[v0.1.0]: https://github.com/rarandeyo/iniad-moocs-cli/releases/tag/v0.1.0
