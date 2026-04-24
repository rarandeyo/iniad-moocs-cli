# Changelog

本プロジェクトの変更履歴です。形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に準拠し、
バージョニングは [Semantic Versioning](https://semver.org/lang/ja/) に従います。

## [Unreleased]

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

[Unreleased]: https://github.com/rarandeyo/iniad-moocs-cli/compare/v0.2.0...HEAD
[v0.2.0]: https://github.com/rarandeyo/iniad-moocs-cli/compare/v0.1.0...v0.2.0
[v0.1.0]: https://github.com/rarandeyo/iniad-moocs-cli/releases/tag/v0.1.0
