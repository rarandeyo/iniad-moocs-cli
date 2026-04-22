# `imoocs` コマンドリファレンス

グローバルフラグはすべて `IMOOCS_*` env に対応。

```
imoocs [--format text|json] [--quiet] [--debug]
       [--year <YYYY>] [--yes]
       <command> ...
```

`--format` は 2 値:

- `text` (default) — 人間向け verb (`doctor` / `auth *` / `setup`) は stdout
  に短い text サマリを出し、stderr に進捗行を流す。agent 向け verb は
  `text` を指定しても**常に整形 JSON envelope** を返す (構造化データを
  text 化する意味がないため)。
- `json` — human-facing verb も含めて**整形 JSON envelope** を返す。
  agent / CI で `doctor` / `auth *` / `setup` を叩くときは常にこれを使う。

`IMOOCS_FORMAT=json` でも global に切替可能。

## 初期セットアップ

### `imoocs setup [--username <u>] [--password-stdin] [--skip-google]`
MOOCs ログイン → Google SSO → doctor を順次実行するファサード。成功時の
envelope:

```jsonc
{
  "success": true,
  "data": {
    "steps": [
      { "step": "authLogin",       "status": "ok",      "details": { "username": "s1f10..." } },
      { "step": "authLoginGoogle", "status": "ok",      "details": { "username": "s1f10..." } },
      { "step": "doctor",          "status": "ok",      "details": { "moocsAuthenticated": true, "googleAuthenticated": true } }
    ],
    "allOk": true
  }
}
```

失敗時は `{success:false, error: {code, message, hint?, details: <SetupReport>}}`。
`error.details.steps` から失敗 step を特定でき、exit code は失敗 step の
コードを流用 (Auth=2 / Validation=3 / Internal=5 等)。

`--skip-google` は `authLoginGoogle` を `status:"skipped"` で省略。
`--password-stdin` は stdin から 1 行読んで keyring に保存し対話を回避。

## 認証

### `imoocs auth login [--username <u>] [--password-stdin]`
MOOCs Keycloak にログイン。対話入力 or `--password-stdin` で非対話。
成功で `$XDG_CONFIG_HOME/imoocs/config.toml` に username、OS keyring に
password、`$XDG_CACHE_HOME/imoocs/cookies.json` にセッション Cookie。

### `imoocs auth login-google`
Google Workspace の SAML ログイン (スライド PDF 取得に必要)。MOOCs 側
の keyring password を流用するので追加入力なし。

### `imoocs auth status` / `imoocs auth logout` / `imoocs auth export`
auth サブコマンドは envelope を返さず、常に text サマリを stdout に出す。
agent は **exit code** と必要なら stderr の失敗行で分岐する。

- `status`: MOOCs / Google SSO / keyring 状態をチェックリストで表示。
  MOOCs 未ログインは exit 2、ログイン済は 0。構造化情報が欲しい場合は
  `imoocs doctor --format json` (`moocsAuthenticated` / `googleAuthenticated`)。
- `logout`: keyring entry と `cookies.json` を削除 (`--keep-config` で
  `config.toml` は温存)。exit 0。
- `export`: `username: ...` / `password: stored in OS keyring (...)` の
  2 行を出す。password 本体は CLI から**絶対に出力されない**。keyring
  から取り出したい場合は OS のキーリング管理ツール (macOS Keychain
  Access / GNOME Seahorse / Windows Credential Manager) を直接使う。

## コース / 授業 / スライド

### `imoocs course list [--year 2026]`
現在 (or 指定) 年度のコース一覧。

### `imoocs course show <courseId>`
`{course, lessons: [...], groups: [{title, lessons: [...]}, ...]}`。
`groups` はサイドバーの章立てを維持。

### `imoocs lesson show <courseId> <lessonId> [--page <pageId>]`
```
[--fetch-slides] [--no-cache] [--with-assignments]
[--url <url>]
```
- `--fetch-slides` で Google Slides を PDF 合成しキャッシュに保存、
  `data.embeds[].localPdfPath` に絶対パスを入れる
- `--with-assignments` で各 `data.assignments[]` を AssignmentDetail に
  展開、`{lesson, assignments: [AssignmentDetail, ...]}` を返す
- `--url <url>` は positional の代わりに MOOCs URL で指定

### `imoocs slide fetch <embedUrl> [--out-dir <cache|tmp|PATH>] [--no-cache]`
任意の pubembed URL を指定して単独で PDF 生成。24h TTL キャッシュ。

**保存先** のデフォルトは `/tmp/imoocs/slides/<sha1(embedUrl)>.pdf` (OS の
一時領域に置いて再起動時に OS 掃除に任せる)。変更する場合:

- **一時的**: `--out-dir <cache|tmp|PATH>` で上書き (この呼び出しのみ)
- **恒久的**: `$XDG_CONFIG_HOME/imoocs/config.toml` に以下を追加
  ```toml
  [slides]
  out_dir = "cache"   # "cache" | "tmp" | 絶対パス
  ```
  - `"cache"` → `$XDG_CACHE_HOME/imoocs/slides/`
  - `"tmp"`   → `/tmp/imoocs/slides/` (default)
  - 絶対パス → そのディレクトリ

`imoocs lesson show --fetch-slides` / `imoocs open --fetch-slides` も同じ
解決ルールに従う (これらには `--out-dir` フラグは無く、config のみ参照)。

## Drive 配布物

INIAD Google Workspace (`@iniad.org`) アカウントに紐づく SAML cookie
(`imoocs auth login-google` で取得) を使って、授業で配布される Drive
フォルダ / ファイルを読み書きする。外部 OAuth 不要。

### `imoocs drive list <folder-url-or-id>`
`/drive/folders/<id>` の直下 items を列挙。MIME が
`application/vnd.google-apps.folder` はサブフォルダ。

`data` shape: `{ folderId, items: DriveItem[], truncated: boolean, fetchedAt }`。
`truncated: true` が返ると 50 件で打ち切られている可能性
(ページング未対応 — v2 で対応予定)。

受け付ける `target`:
- `https://drive.google.com/drive/folders/<id>` URL
- 生の folder ID

### `imoocs drive fetch <file-url-or-id> [--out <path>] [--no-cache]`
単一ファイルを `$XDG_CACHE_HOME/imoocs/drive/<fileId>.<ext>` に保存。
Content-Disposition の filename から拡張子を決定、欠損時は mime_guess
にフォールバック。`--out` で追加コピー、`--no-cache` で 24h TTL を
無視して再 DL。

`data` shape: `{ fileId, filename, mime, localPath, sizeBytes, fetchedAt, fromCache }`。

受け付ける `target`:
- `https://drive.google.com/file/d/<id>/(view|preview)?` URL
- `https://drive.google.com/uc?export=download&id=<id>` (旧ホスト)
- `https://drive.usercontent.google.com/download?id=<id>...` (新ホスト)
- 生の file ID

**未対応**: Google ネイティブ型 (Docs/Sheets/Slides の mime
`application/vnd.google-apps.*`) は `drive.usercontent.google.com` が
空 HTML を返すため本コマンドでは exit 1 (`API_ERROR`) で弾く。
pubembed のスライドなら `imoocs slide fetch` が使える。v2 で
`--export pdf|docx|pptx|xlsx` を追加予定。

## 課題

### `imoocs assignment list <courseId> [--lesson <id>] [--status <filter>]`
`<filter>`: `pending | submitted | closed | graded | network | error |
nonpublic | open | all` (デフォルト `all`、`open` = Pending+Submitted)。
各 summary に `derivedStatus` と `lessonId` / `pageId` が付く。

### `imoocs assignment show <courseId> <problemId> [--lang ja|en]`
または `--url <url>` (lesson URL を指定した場合、ページ上の単一
`.problem-container` を選択、複数あるとエラー)。

### `imoocs assignment answer <c> <p> --data <json>`
`--data '{"p1":"x"}'` / `--data @file.json` / `--data -` (stdin) の 3 形式。
`submitted:false` で下書き (force なし)。

### `imoocs assignment submit <c> <p> [--data ...]`
「確定したい」という意思表示。実際に `force=true` で PUT されるかは
config `[assignment] confirm` による (下表)。data 省略時はサーバ現行値の
まま確定する意思を示す。`-y/--yes` フラグは存在しない。

### `imoocs assignment upload <c> <p> --pid <pid> <file> [--force]`
multipart POST。`--force` は「確定したい」意思表示で、実際のサーバ送信
`force` は config `[assignment] confirm` で決まる。`--force` なしは常に
下書き (force=false)。

### 確定モード表 (`[assignment] confirm`)

| mode | `submit` / `upload --force` |
|---|---|
| 未設定 | exit 3 `"config assignment.confirm is not set"` |
| `"auto"` | 常に `force=true` (AI agent を信頼) |
| `"confirm"` | TTY で `y` を押したときだけ `force=true`。n / 非TTY はすべて `force=false` で下書き保存 (stderr に notice) |

`confirm` モードでは非 TTY から確定させる手段が無いのが中核。AI agent
経由の最終提出は人間レビューを通るまで先延ばしされる。設定は
`imoocs setup` の [3/4] プロンプトで選ぶか、`$XDG_CONFIG_HOME/imoocs/config.toml` を直接編集:

```toml
[assignment]
confirm = "auto"    # または "confirm"
```

## URL 1 本で操作

### `imoocs open <url> [--fetch-slides] [--no-cache] [--lang ja|en]`
MOOCs URL を MoocsPath::parse し、`OpenResult`:
- `/courses[/<year>]` → `Courses { year, courses }`
- `/courses/<y>/<c>` → `Course(CourseDetail)`
- `/courses/<y>/<c>/<l>[/<p>]` → `Lesson(LessonWithAssignments)`
  (lesson 内容 + 各 assignment の詳細を合成)

## 横断

### `imoocs doctor`
`{version, moocsAuthenticated, googleAuthenticated, configDir, dataDir,
cacheDir, username}`。exit 2 if not authenticated.

### `imoocs completion <bash|zsh|fish|powershell|elvish>`
補完スクリプトを stdout に書き出す。

Skill 配信は CLI 側のサブコマンドではなく、GitHub CLI の
`gh skill install rarandeyo/iniad-moocs-cli skills/imoocs --agent <host> --scope <user|project>`
に委譲する (agentskills.io open standard 準拠; Claude Code / Cursor /
Copilot / Codex / Gemini CLI / Antigravity 共通)。

## Exit code 一覧

| code | 意味 |
|---|---|
| 0 | success |
| 1 | API error (server 4xx/5xx) |
| 2 | authentication (AUTH_EXPIRED, GOOGLE_AUTH_REQUIRED) |
| 3 | validation (引数 or JSON 不正) |
| 4 | not found |
| 5 | internal (バグ想定) |
| 6 | network error (接続/タイムアウト) |
| 7 | NETWORK_RESTRICTED (学内 IP 限定; 出席確認と一部のみ) |
