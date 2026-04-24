# Changelog

本プロジェクトの変更履歴です。形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に準拠し、
バージョニングは [Semantic Versioning](https://semver.org/lang/ja/) に従います。

## [Unreleased]

## [0.2.0] - 2026-04-25

### Added
- `imoocs reset` サブコマンドを追加。`--scope auth|config|cache|drafts|all`
  (複数指定 / CSV 可) で各領域をまとめて初期化できる (#36f631e)。
  - 対話 TTY は default No 確認、非 TTY では `--yes` 必須、`--dry-run` 対応。
  - `slides_dir` の実削除は `<cache_dir>/slides` と `/tmp/imoocs/slides` のみ許可し、
    ユーザ指定任意パスは refuse + skip で巻き込み事故を防ぐ。
  - keyring 失敗時は `config.toml` を残し retry 可能な状態を維持する。
- `course-drive-folders` スキーマに `unresolvedReason` フィールドを新設
  (`deferred` / `not-offered` / `pending-folder` / `needs-user-input`) し、
  `/imoocs-drive-setup` 再走時の挙動を状態区分ごとに分岐できるようにした (#3)。
- `skills/imoocs-drive-setup/SKILL.md` に INIAD 命名規則の正規化規範
  (NFKC + ローマ数字↔I/V/X 境界判定 + 中黒削除 + `/` 旧新名分割 + 比較キー /
  トークン列の 2 系列分離) と、1:1 / 1:N / N:1 / 旧新名併記 / `1:0+reason`
  の 4 ケース別テンプレを追加 (#3)。

### Changed
- **Breaking (設定スキーマ)**: `course-drive-folders.json` の
  `courses[].driveFolderId: String` を `courses[].driveFolders: Vec<DriveFolderRef>`
  に変更し、1 コース複数フォルダ (概論+演習) / 複数コース 1 フォルダ共有
  (デザイン理論等) の N:M 対応表現を可能にした (#3)。
  `/imoocs-drive-setup` で新スキーマへ自動再生成される。
- `auth logout` の責務を keyring 認証情報と cookies の削除に限定。
  `config.toml` は削除せず残すよう挙動を整理し、`reset --scope auth` と
  等価にして住み分けを明確化した (#36f631e)。
- README の agent skill (`skills/imoocs/SKILL.md`) インストール / セットアップ
  手順を刷新 (#825be0b)。
- Release workflow (cargo-dist) の前段に `ci.yml` (fmt / clippy / test) を
  gate として挟み、CI 失敗時はリリースを止めるようにした (#2)。
- npm publish を **OIDC trusted publishing** に移行。長命の
  `NPM_TOKEN` (classic token) を廃止し、GitHub Actions の OIDC で
  npm registry に対して trusted publish する方式に切り替えた。併せて
  published package の `engines.node` を `>=22` に正規化、`repository`
  を object form で正規化して `--provenance` の要件を満たす (#4)。

### Removed
- **Breaking (CLI フラグ)**: `auth logout --keep-config` フラグを削除。
  `config.toml` を残すのが既定挙動になったため冗長化したので廃止。
  代わりに `imoocs reset --scope auth` を使用すれば同等の動作となる (#36f631e)。
- **Breaking (doctor スキーマ)**: `imoocs doctor` から agent skill の検出機能
  (`DoctorReport.skills` / `SkillDetectionReport` / `SkillDetectionMethod`)
  を削除。マルチホスト (Claude Code / Codex / Copilot / Cursor / Gemini /
  Antigravity) の追従コストに対し、false negative (skill を入れているのに `⚠`)
  の方が支配的だったため機能ごと撤去 (#5)。`googleAuthenticated` 等の他の
  doctor フィールドには影響しない。

## [0.1.0] - 2026-04-25

初回安定リリース。詳細は GitHub Releases を参照。

[Unreleased]: https://github.com/rarandeyo/iniad-moocs-cli/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/rarandeyo/iniad-moocs-cli/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/rarandeyo/iniad-moocs-cli/releases/tag/v0.1.0
