# `imoocs` コマンドリファレンス

グローバルフラグはすべて `IMOOCS_*` env に対応。

```
imoocs [--format json|pretty|ndjson] [--quiet] [--debug]
       [--year <YYYY>] [--yes]
       <command> ...
```

## 認証

### `imoocs auth login [--username <u>] [--password-stdin]`
MOOCs Keycloak にログイン。対話入力 or `--password-stdin` で非対話。
成功で `$XDG_CONFIG_HOME/imoocs/config.toml` に username、OS keyring に
password、`$XDG_CACHE_HOME/imoocs/cookies.json` にセッション Cookie。

### `imoocs auth login-google`
Google Workspace の SAML ログイン (スライド PDF 取得に必要)。MOOCs 側
の keyring password を流用するので追加入力なし。

### `imoocs auth status` / `imoocs auth logout` / `imoocs auth export`
`status` は `{moocsAuthenticated, googleAuthenticated, username, ...}` を
返す。未認証時は exit 2。`logout` は keyring entry と cookies.json を削除。
`export` は config と keyring の状態表示 (デフォルト masked)。

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

### `imoocs slide fetch <embedUrl> [--out <path>] [--no-cache]`
任意の pubembed URL を指定して単独で PDF 生成。24h TTL キャッシュ。

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

### `imoocs assignment submit <c> <p> [--data ...] --yes`
`--yes` 必須。force=true で PUT、`submitted:true`。data 省略時は
サーバ現行値のまま確定。

### `imoocs assignment upload <c> <p> --pid <pid> <file> [--force]`
multipart POST。`--force` で同時確定。

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

### `imoocs skill install [--user|--project]`
埋め込み済みの `skills/imoocs/` を `~/.claude/skills/imoocs/` (user) か
カレントの `.claude/skills/imoocs/` (project) にコピー。

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
| 7 | NETWORK_RESTRICTED (学内 IP 限定) |
