# imoocs CLI Reference

この文書は、現在の `imoocs` 実装と `--help` 出力をもとにした完全リファレンス。
README の概要より細かく、以下をまとめる。

- 全コマンドとサブコマンド
- 全公開オプションと環境変数
- `config.toml` / XDG 保存先 / keyring / draft / 補助 TOML
- 有効な組み合わせ、無効な組み合わせ、実装上は受理されるが実質意味が薄い組み合わせ
- `--help` に出ない hidden surface と、help 文面と実装の差分

実装は `crates/imoocs-cli` と `crates/imoocs-core` を基準に読んでいるため、将来の変更で古くなる可能性がある。

## 1. 読み方

### 1.1 正式名、alias、前方一致

トップレベルの visible alias:

| 正式名 | alias |
|---|---|
| `course` | `c` |
| `lesson` | `l` |
| `assignment` | `a` |
| `slide` | `s` |
| `drive` | `d` |

サブコマンドの visible alias:

| 正式名 | alias |
|---|---|
| `course list` | `course ls` |
| `assignment list` | `assignment ls` |
| `assignment drafts list` | `assignment drafts ls` |

さらに top-level では `infer_subcommands = true` が有効なので、一意に決まる前方一致も使える。

| 入力 | 結果 |
|---|---|
| `imoocs ass --help` | `imoocs assignment --help` として通る |
| `imoocs a li --help` | `imoocs assignment list --help` として通る |
| `imoocs co --help` | `course` と `completion` が衝突するので失敗 |

前方一致は「一意なら通る」。曖昧なら失敗する。

### 1.2 built-in `help` / `version`

`clap` 標準の `-h/--help` と `-V/--version` も使える。

| 呼び方 | 出力 |
|---|---|
| `imoocs -h` | plain text help |
| `imoocs --help` | plain text help |
| `imoocs -V` | plain text version |
| `imoocs --version` | plain text version |
| `imoocs version` | JSON envelope を返す CLI サブコマンド |

つまり `imoocs version` と `imoocs -V` は別物。

## 2. コマンドツリー

```text
imoocs
├── version
├── doctor
├── setup
├── auth
│   ├── login
│   ├── login-google
│   ├── logout
│   ├── status
│   └── export
├── course (c)
│   ├── list (ls)
│   └── show
├── lesson (l)
│   └── show
├── assignment (a)
│   ├── list (ls)
│   ├── show
│   ├── submit
│   ├── upload
│   ├── push
│   └── drafts
│       ├── list (ls)
│       ├── show
│       └── clear
├── slide (s)
│   └── fetch
├── drive (d)
│   ├── list
│   ├── search
│   ├── fetch
│   └── folders
├── open
├── reset
└── completion
    ├── generate
    └── install
```

## 3. グローバルオプション

全サブコマンドで受け取れるオプション:

| Option | Env | Default | 実効 |
|---|---|---|---|
| `--format <text|json>` | `IMOOCS_FORMAT` | `text` | 出力形式と tracing formatter を切り替える。ただしコマンドによっては無視される |
| `--no-progress` | `IMOOCS_NO_PROGRESS` | false | 現行コードでは実効箇所が見当たらず、実質 no-op |
| `-q`, `--quiet` | `IMOOCS_QUIET` | false | tracing を error のみに絞る。直接 `eprintln!` している進捗や prompt は消えない |
| `--debug` | `IMOOCS_DEBUG` | false | tracing を debug に上げる。`--quiet` より優先 |
| `--year <u32>` | `IMOOCS_YEAR` | なし | year を URL 解決で持たないコマンド群に適用する |

### 3.1 `--format` の実効マトリクス

| コマンド群 | `--format text` | `--format json` |
|---|---|---|
| `version`, `course *`, `lesson show`, `assignment *`, `slide fetch`, `drive list`, `drive fetch`, `open` | 常に JSON envelope | 常に JSON envelope |
| `doctor`, `drive search`, `drive folders` | 人間向け text | JSON envelope |
| `setup` 成功時 | 人間向け text | JSON envelope |
| `setup` 失敗時 | JSON failure envelope | JSON failure envelope |
| `auth *` | text 専用。`--format` を無視 | text のまま |
| `completion *` | text 専用。`--format` を無視 | text のまま |

`setup` は成功時だけ `emit_success_text` を使い、失敗時は常に failure envelope を返す点が少し特殊。

### 3.2 `--debug` と `--quiet`

tracing 初期化の優先順位は次の通り。

1. `--debug`
2. `--quiet`
3. `--format json` なら info
4. それ以外は warn

つまり `--debug --quiet` を同時に付けると debug が勝つ。

ただし `auth` や `setup` は progress や結果の一部を `println!` / `eprintln!` で直接出しているので、`--quiet` はそれらを抑制しない。

### 3.3 `--year` の実効マトリクス

| コマンド | `--year` の意味 |
|---|---|
| `course list`, `course show` | 使用される。未指定なら latest year を解決 |
| `lesson show` の positional 形式 | 使用される。未指定なら latest year |
| `lesson show --url ...` | URL 内の year を使うので無視される |
| `assignment list` | 使用される。未指定なら latest year |
| `assignment show/submit/upload/push/drafts show/drafts clear` の positional 形式 | 使用される。未指定なら latest year |
| `assignment ... --url ...` | URL 内の year を使うので無視される |
| `open <url>` | URL から決まるので無視される |
| `auth *`, `setup`, `doctor`, `slide *`, `drive *`, `completion *`, `version` | 実質無意味 |

## 4. 永続設定と保存先

### 4.1 XDG ディレクトリ

| 用途 | 既定パス |
|---|---|
| config | `$XDG_CONFIG_HOME/imoocs/` |
| data | `$XDG_DATA_HOME/imoocs/` |
| cache | `$XDG_CACHE_HOME/imoocs/` |
| state | `$XDG_STATE_HOME/imoocs/` |

`state` 概念がない OS では `data_dir/state` に fallback する。

### 4.2 実際に使うファイルとディレクトリ

| パス | 用途 | 書き込み元 |
|---|---|---|
| `$XDG_CONFIG_HOME/imoocs/config.toml` | 非機微設定 | `auth login`, `setup`, 手編集 |
| `$XDG_CONFIG_HOME/imoocs/course-drive-folders.toml` | コースと Drive フォルダの対応 | 読み取り専用。主に `imoocs-drive-setup` skill が書く |
| `$XDG_CACHE_HOME/imoocs/cookies.json` | MOOCs / Google の cookie jar | 認証処理、session 保存 |
| `$XDG_CACHE_HOME/imoocs/drive/` | Drive ダウンロード cache | `drive fetch` |
| `$XDG_CACHE_HOME/imoocs/slides/` | slide PDF cache | `slides.out_dir = "cache"` 時など |
| `/tmp/imoocs/slides/` | slide PDF の既定保存先 | `slides.out_dir` 未指定時 |
| `$XDG_STATE_HOME/imoocs/drafts/` | staged draft | `assignment submit/upload` in `confirm` mode |
| OS keyring | password | `auth login`, `setup` |

補足:

- `Paths::credentials_file()` は定義されているが、現行 CLI では使っていない。
- secret は `config.toml` ではなく keyring と `cookies.json` に置く設計。

### 4.3 `config.toml`

有効キーは現状この 3 つだけ。

```toml
username = "s1f10XXXXXXX"

[slides]
out_dir = "cache" # "cache" / "tmp" / 絶対パス

[assignment]
confirm = "confirm" # "auto" / "confirm"
```

#### `username`

- `auth login` 成功時に保存される
- `setup` でも同じロジックを使う
- 主用途は次回以降の既定 username

#### `[slides].out_dir`

有効値:

| 値 | 解決先 |
|---|---|
| `"cache"` | `$XDG_CACHE_HOME/imoocs/slides/` |
| `"tmp"` | `/tmp/imoocs/slides/` |
| 絶対パス | そのまま |

無効:

- 相対パス
- `"cache"` / `"tmp"` / 絶対パス以外

優先順位:

1. `slide fetch --out-dir`
2. `config.toml [slides].out_dir`
3. 組み込み既定値 `"tmp"`

この設定を参照するのは `slide fetch`、`lesson show` / `open <lesson-url>` の既定 slide fetch (抑制は `--no-fetch-slides`)、`--no-cache` 指定時の強制再取得。

#### `[assignment].confirm`

有効値:

| 値 | `submit` / `upload` | `push` |
|---|---|---|
| 未設定 | Validation error | Validation error |
| `"auto"` | 即サーバ確定 | staged draft があれば push |
| `"confirm"` | local draft に stage のみ | TTY + 確認 prompt 後に確定 |

CLI / env でこの値を直接 override する方法はない。設定ソースは `config.toml` のみ。

### 4.4 `course-drive-folders.toml`

CLI からは読み取り専用 (書き込みは `imoocs-drive-setup` skill が担当)。schema は N:M を許容: 1 コースに複数 Drive フォルダ (1:N) は `[[courses.driveFolders]]` を複数並べ、複数コースが同一フォルダを共有 (N:1) は同じ `id` を各 entry に書く。概形は次の通り。

```toml
driveRootFolderId = "..."

# 1:1 (典型)
[[courses]]
year = 2026
courseId = "INI301"
name = "..."
matchedAt = "2026-04-25"
matchStrategy = "exact" # exact / partial / user-confirmed / unresolved
[[courses.driveFolders]]
id = "..."
url = "https://drive.google.com/drive/folders/..."

# 1:N (1 コースに複数フォルダ)
[[courses]]
year = 2026
courseId = "COT101"
name = "コンピュータ・サイエンス概論 I & 演習 I"
matchStrategy = "user-confirmed"
[[courses.driveFolders]]
id = "..."
url = "..."
[[courses.driveFolders]]
id = "..."
url = "..."

# Unresolved (理由分類付き)
[[courses]]
year = 2026
courseId = "CV101"
name = "..."
matchStrategy = "unresolved"
unresolvedReason = "not-offered" # deferred / not-offered / pending-folder / needs-user-input
```

`unresolvedReason` は `matchStrategy = "unresolved"` の理由分類。再走時の挙動 (`not-offered` はスキップ、`deferred` 等は再解決を試みる) を分けるために使う。`drive folders` と `doctor` がこれを読む。

## 5. 出力と終了コード

### 5.1 JSON envelope

agent 向け verb は pretty JSON envelope を返す。

```json
{ "success": true, "data": { "...": "..." } }
{ "success": false, "error": { "code": "...", "message": "...", "hint": "..." } }
```

### 5.2 Exit code

| Exit code | 意味 | 代表的な error code |
|---|---|---|
| 0 | Success | - |
| 1 | API | `API_ERROR` |
| 2 | Auth | `AUTH_EXPIRED` |
| 3 | Validation | `VALIDATION_ERROR` |
| 4 | NotFound | `NOT_FOUND` |
| 5 | Internal / Parse / I/O | `INTERNAL_ERROR`, `PARSE_ERROR` |
| 6 | Network | `NETWORK_ERROR` |
| 7 | NetworkRestricted | `NETWORK_RESTRICTED` |
| 8 | NonPublic | `NON_PUBLIC` |

## 6. コマンド別リファレンス

## 6.1 `version`

構文:

```sh
imoocs version
imoocs -V
imoocs --version
```

組み合わせ:

| 呼び方 | 結果 |
|---|---|
| `imoocs version` | JSON envelope |
| `imoocs -V` / `--version` | plain text version |
| `imoocs version --format text|json` | どちらでも JSON envelope |

## 6.2 `doctor`

構文:

```sh
imoocs doctor
```

確認内容:

- MOOCs 認証
- Google SSO
- `assignment.confirm`
- completion 配置
- `imoocs` / `imoocs-drive-setup` skill 検出
- `course-drive-folders.toml`
- Quick start 完了判定

skill 検出は:

1. `gh skill list --json name`
2. 失敗したら `~/.claude/skills/<name>/SKILL.md`

の順。

組み合わせ:

| 組み合わせ | 結果 |
|---|---|
| `doctor --format text` | 人間向けレポート |
| `doctor --format json` | JSON envelope |
| `doctor` で MOOCs 未ログイン | exit 2 |
| `doctor` で config 壊れている | 現実装では `Config::load(...).unwrap_or_default()` のため既定値扱いで続行 |

completion の検出は `$SHELL` 依存。未設定または `bash/zsh/fish` 以外だと「未検出」警告になる。

## 6.3 `setup`

構文:

```sh
imoocs setup [--username <u>] [--password-stdin] [--skip-google] [--install-completion]
```

処理順:

1. MOOCs login
2. Google login
3. `assignment.confirm`
4. completion install

重要な優先順位:

- username: `--username` > `IMOOCS_USERNAME` > `config.username` > prompt
- password: `--password-stdin` > keyring > prompt
- confirm mode: 既存 `config.toml` にあればそれを使用、なければ prompt

組み合わせマトリクス:

| 組み合わせ | 結果 |
|---|---|
| `setup --skip-google` | step 2 を skip |
| `setup --install-completion` | step 4 を無条件で試行 |
| `setup` + text mode + TTY | completion install するか Confirm prompt |
| `setup --format json` | JSON envelope で返すが、username/password/confirm mode の prompt は残る |
| `setup --format json` かつ `--install-completion` なし | completion install prompt は出さず step 4 を skip |
| `setup` + 非 TTY + `assignment.confirm` 未設定 | confirm mode prompt が必要になり失敗しうる |
| `setup` + 非 TTY + username 未確定 | username prompt が必要になり失敗しうる |
| `setup` + 非 TTY + password 未確定かつ `--password-stdin` なし | password prompt が必要になり失敗しうる |

実装上の注意:

- step 1, 2, 3 の失敗は setup 全体の failure になる
- step 4 の completion install 失敗は report に error として残るが、setup 全体は成功扱いのまま続く
- 成功時は `nextSteps` に skill install と Drive setup の誘導が入る
- 失敗時は text mode でも stdout は JSON failure envelope

## 6.4 `auth`

### `auth login`

構文:

```sh
imoocs auth login [--username <u>] [--password-stdin]
```

優先順位:

| 項目 | 優先順位 |
|---|---|
| username | `--username` > `IMOOCS_USERNAME` > `config.username` > prompt |
| password | `--password-stdin` > keyring > prompt |

組み合わせ:

| 組み合わせ | 結果 |
|---|---|
| `--password-stdin` | stdin 全体を読み、末尾改行だけ落として keyring に保存 |
| username あり + keyring password あり | prompt なし |
| username あり + keyring password なし | password prompt |
| username なし + config.username あり | config 値を使う |
| username なし + config.username なし | username prompt |
| 認証失敗 | 保存済み keyring password を削除して exit 2 |

副作用:

- 成功すると `config.username` を保存
- password は OS keyring に保存
- session cookie は `cookies.json` に保存される

`auth *` は `--format` を無視し、常に text。

### `auth login-google`

構文:

```sh
imoocs auth login-google
```

前提:

- `config.username` がある
- keyring に password がある

組み合わせ:

| 状態 | 結果 |
|---|---|
| username なし | Validation error |
| keyring password なし | Validation error |
| 両方あり | Google SSO session を確立 |

### `auth logout`

構文:

```sh
imoocs auth logout
```

挙動:

- OS keyring に保存された password を破棄する
- `cookies.json` を削除し in-memory cookie store もクリアする
- `config.toml` (username / preference) は **残す** — 再 login 時に再入力せず済む

`config.toml` も含めてすべて消したい場合は `imoocs reset --scope config`
(config だけ) あるいは `imoocs reset --scope all` (完全リセット) を使う。

### `auth status`

構文:

```sh
imoocs auth status
```

出力:

- MOOCs login の有無
- Google SSO の有無
- keyring に password があるか
- `cookies.json`, `config.toml` のパス

終了コード:

| 状態 | exit |
|---|---|
| MOOCs ログイン済み | 0 |
| MOOCs 未ログイン | 2 |

Google SSO が切れていても、MOOCs が生きていれば exit 0 のまま。

### `auth export`

構文:

```sh
imoocs auth export
```

表示:

- `username: ...`
- `password: stored in OS keyring ...` または `not stored`

password 本文は出さない。

## 6.5 `course`

### `course list`

構文:

```sh
imoocs course list
imoocs course ls
imoocs c list
```

組み合わせ:

| 組み合わせ | 結果 |
|---|---|
| `--year <y>` あり | その year |
| `--year` なし | latest year を解決 |
| `--format text|json` | 常に JSON envelope |

### `course show`

構文:

```sh
imoocs course show <COURSE_ID>
```

組み合わせ:

| 組み合わせ | 結果 |
|---|---|
| `--year <y>` あり | その year の course を見る |
| `--year` なし | latest year を解決 |

`COURSE_ID` は必須。local option はない。

## 6.6 `lesson show`

構文:

```sh
imoocs lesson show <COURSE_ID> <LESSON_ID> [--page <PAGE>]
imoocs lesson show --url <LESSON_OR_PAGE_URL>
```

軸:

- 対象指定: positional か `--url`
- slide 取得: 既定で best-effort fetch。`--no-fetch-slides` で抑制、`--no-cache` で cache 無視の強制再取得
- assignment 展開: 既定で全課題を `AssignmentDetail` に展開。`--no-assignments` で抑制
- 言語: `--lang ja|en` (assignment 展開時に効く。`--no-assignments` 時は無視)

組み合わせマトリクス:

| 組み合わせ | 結果 |
|---|---|
| positional だけ | `COURSE_ID`, `LESSON_ID`, 任意で `--page` |
| `--url` だけ | URL から `course/lesson/page` を解決 |
| positional + `--url` | conflict。`page` も含めて不可 |
| (既定) | `{lesson, assignments: [AssignmentDetail...]}` を返し、embed 内 Google Slides は best-effort PDF 取得 |
| `--no-fetch-slides` | Google Slides の PDF 取得をスキップ。`embeds[*].localPdfPath` は `null`、`fetchStatus` 省略 |
| `--no-assignments` | `assignments: []` で返す (ページ本文だけが欲しい軽量モード) |
| `--no-fetch-slides --no-assignments` | markdown + embeds (URL メタのみ) だけの最軽量 |
| `--no-cache` だけ | 既定 fetch の cache を無視して強制再取得 (`--no-fetch-slides` 併用時は無視) |
| `--lang ja|en` | assignment detail の言語を選ぶ (既定 `ja`) |
| `--lang` + `--no-assignments` | `--lang` は受理されるが assignment 展開しないので効果なし |
| positional + `--year` | `--year` を使用 |
| `--url` + `--year` | URL の year を使うため `--year` は無視 |

Slide fetch 失敗時 (best-effort):

- Google SSO 未ログイン → `fetchStatus: "skipped"`、exit は 0 維持、stderr に warn 1 行 (`--quiet` で抑制)
- ネットワーク障害など → `fetchStatus: "failed"`、同様に exit 0 + warn
- いずれも `markdown` / `assignments` は正常、他の embed も独立に処理される

URL 制約:

- `--url` は lesson URL か page URL のみ受理
- course URL や assignment URL は不可

出力:

- 常に JSON envelope (`{lesson, assignments}` の固定 shape)

## 6.7 `assignment`

`assignment` は `config.assignment.confirm` の影響を最も強く受ける。

### `assignment list`

構文:

```sh
imoocs assignment list <COURSE_ID> [--lesson <LESSON_ID>] [--status <STATUS>]
```

`STATUS`:

- `pending`
- `submitted`
- `closed`
- `graded`
- `network`
- `error`
- `nonpublic`
- `open`
- `all`

組み合わせ:

| 組み合わせ | 結果 |
|---|---|
| `--lesson` なし | コース全体 |
| `--lesson <id>` | 該当 lesson 配下だけ |
| `--status all` | 無フィルタ |
| `--status open` | `Pending` と `Submitted` の合算 |
| `--year <y>` | その year |
| `--year` なし | latest year |

### `assignment show`

構文:

```sh
imoocs assignment show <COURSE_ID> <PROBLEM_ID> [--lang <ja|en>]
imoocs assignment show --url <LESSON_OR_PAGE_URL> [--lang <ja|en>]
```

組み合わせ:

| 組み合わせ | 結果 |
|---|---|
| positional | `COURSE_ID` + `PROBLEM_ID` を直接使う |
| `--url` | lesson/page から problem を自動解決 |
| positional + `--url` | conflict |
| `--lang ja|en` | detail の言語選択 |
| positional + `--year` | `--year` 使用 |
| `--url` + `--year` | URL year を使うので `--year` 無視 |

`--url` の実際の制約:

- lesson/page URL のみ
- ページ内 assignment 数がちょうど 1 個のときだけ成功
- 0 個なら `NOT_FOUND`
- 2 個以上なら `VALIDATION_ERROR`

help 文面には「最初の `.problem-container` を採用」と読める説明があるが、実装は「ちょうど 1 個のときだけ採用」。

### `assignment submit`

構文:

```sh
imoocs assignment submit <COURSE_ID> <PROBLEM_ID> --data <JSON|@PATH|->
imoocs assignment submit --url <LESSON_OR_PAGE_URL> --data <JSON|@PATH|->
```

`--data` の入力形式:

| 形式 | 意味 |
|---|---|
| `'{"pid":"value"}'` | その場で JSON object を渡す |
| `@answers.json` | ファイルから読む |
| `-` | stdin から読む |

制約:

- `--data` は JSON object 必須
- array や scalar は不可
- key は pid、value は任意 JSON value

`confirm` マトリクス:

| `assignment.confirm` | 結果 |
|---|---|
| 未設定 | Validation error |
| `auto` | 即 `put_answers(..., force=true)` |
| `confirm` | draft に stage し、サーバ未送信 |

位置指定と URL 指定の組み合わせは `assignment show` と同じ。

### `assignment upload`

構文:

```sh
imoocs assignment upload <COURSE_ID> <PROBLEM_ID> <FILE> --pid <PID>
imoocs assignment upload --url <LESSON_OR_PAGE_URL> <FILE> --pid <PID>
```

`FILE` は help 上は optional に見えるが、実装上は実質必須。省略すると runtime validation で落ちる。

`confirm` マトリクス:

| `assignment.confirm` | 結果 |
|---|---|
| 未設定 | Validation error |
| `auto` | 即 `post_file(..., force=true)` |
| `confirm` | draft の `files[pid]` に絶対パスを stage |

組み合わせ:

| 組み合わせ | 結果 |
|---|---|
| `--pid <PID>` 必須 | file field を指定 |
| `FILE` + `--url` | 可 |
| `FILE` なし | runtime validation error |
| relative `FILE` + `confirm` | `canonicalize()` して絶対パスを保存 |
| relative `FILE` + `auto` | そのまま API 呼び出しに使う |

### `assignment push`

構文:

```sh
imoocs assignment push <COURSE_ID> <PROBLEM_ID>
imoocs assignment push --url <LESSON_OR_PAGE_URL>
```

前提:

- `assignment.confirm` が設定済み
- TTY である
- 対象 draft が存在する

`push` マトリクス:

| 状態 | 結果 |
|---|---|
| `assignment.confirm` 未設定 | Validation error |
| 非 TTY | Validation error |
| draft なし | `NOT_FOUND` |
| draft あり + prompt で `y` | answers/files を順次確定送信 |
| draft あり + prompt で `n` | Validation error。draft は残る |
| push 中に API/Network 失敗 | draft は残る。再実行で resume 可 |
| upload-only draft | `put_answers` をスキップし files のみ push |

注意:

- `auto` mode でも `push` 自体は使える
- `submit/upload` で stage された draft がない限り push しても意味はない
- URL 指定時は、`assignment show` と同じく lesson/page 内 assignment がちょうど 1 個必要

### `assignment drafts list`

構文:

```sh
imoocs assignment drafts list
imoocs assignment drafts ls
```

draft 一覧を返す。常に JSON envelope。

### `assignment drafts show`

構文:

```sh
imoocs assignment drafts show <COURSE_ID> <PROBLEM_ID>
imoocs assignment drafts show --url <LESSON_OR_PAGE_URL>
```

対象解決ルールは `assignment push` と同じ。

### `assignment drafts clear`

構文:

```sh
imoocs assignment drafts clear <COURSE_ID> <PROBLEM_ID>
imoocs assignment drafts clear --url <LESSON_OR_PAGE_URL>
imoocs assignment drafts clear --all
```

組み合わせ:

| 組み合わせ | 結果 |
|---|---|
| positional | 単一 draft を削除 |
| `--url` | URL から単一 draft を特定して削除 |
| `--all` | drafts directory 内を全削除 |
| `--all` + positional | conflict |
| `--all` + `--url` | conflict |

`--all` 時だけ year 解決は不要。単一指定時は positional なら `--year`、URL なら URL 内 year を使う。

実装差分:

- `resolve_key()` の複数 assignment エラーメッセージは `--problem-id` を使えと言うが、現行 CLI にその flag はない。実際には positional `<COURSE_ID> <PROBLEM_ID>` を使う。

## 6.8 `slide fetch`

構文:

```sh
imoocs slide fetch <EMBED_URL> [--out-dir <cache|tmp|ABS>] [--no-cache]
```

hidden surface:

```sh
imoocs slide fetch <EMBED_URL> --dump-svgs <DIR>
```

`--dump-svgs` は help に出ないデバッグ用オプション。

`out_dir` マトリクス:

| 入力 | 結果 |
|---|---|
| `--out-dir cache` | cache 配下 |
| `--out-dir tmp` | `/tmp/imoocs/slides/` |
| `--out-dir /abs/path` | その絶対パス |
| `--out-dir relative/path` | Validation error |
| `--out-dir` なし | `config.toml [slides].out_dir` > `"tmp"` |

cache マトリクス:

| 組み合わせ | 結果 |
|---|---|
| 既定 | cache を使う |
| `--no-cache` | 強制再取得 |

`--year` は無意味。

## 6.9 `drive`

### `drive list`

構文:

```sh
imoocs drive list <TARGET>
```

受け取れる `TARGET`:

- `https://drive.google.com/drive/folders/<id>`
- bare folder id

組み合わせ:

| 入力 | 結果 |
|---|---|
| folder URL | folder id を抽出して list |
| bare id | このコマンドでは folder とみなして list |
| file URL | Validation error。`drive fetch` を使え |

### `drive search`

構文:

```sh
imoocs drive search <NAME> [--exact]
```

組み合わせ:

| 組み合わせ | 結果 |
|---|---|
| 既定 | partial match |
| `--exact` | exact match |
| `--format text` | 人間向け表 |
| `--format json` | JSON envelope |

### `drive fetch`

構文:

```sh
imoocs drive fetch <TARGET> [--out <PATH>] [--no-cache]
```

受け取れる `TARGET`:

- `https://drive.google.com/file/d/<id>/...`
- `https://drive.google.com/uc?...&id=<id>`
- `https://drive.usercontent.google.com/download?id=<id>`
- bare file id

組み合わせ:

| 組み合わせ | 結果 |
|---|---|
| bare id | このコマンドでは file とみなして fetch |
| folder URL | Validation error。`drive list` を使え |
| `--out <PATH>` | cache に保存した後、そのパスにもコピー |
| `--no-cache` | 24h cache を無視して再取得 |

### `drive folders`

構文:

```sh
imoocs drive folders
```

組み合わせ:

| 条件 | 結果 |
|---|---|
| `course-drive-folders.toml` あり | 登録内容を表示 |
| ファイルなし | 「未登録」案内 |
| `--format text` | 人間向け text |
| `--format json` | JSON envelope |

`drive list/fetch` は常に JSON envelope だが、`search/folders` は text/json 切替という点に注意。

## 6.10 `open`

構文:

```sh
imoocs open <URL> [--no-fetch-slides] [--no-cache] [--lang <ja|en>]
```

ルーティング:

| URL 種別 | 結果 |
|---|---|
| `/courses` | latest year の course list |
| `/courses/<year>` | 指定 year の course list |
| `/courses/<year>/<courseId>` | course detail |
| `/courses/<year>/<courseId>/<lessonId>` | lesson + assignments (slide PDF も best-effort fetch) |
| `/courses/<year>/<courseId>/<lessonId>/<pageId>` | page + assignments (slide PDF も best-effort fetch) |
| それ以外 | Validation error |

組み合わせ:

| 組み合わせ | 結果 |
|---|---|
| (既定) + lesson/page URL | `OpenResult::Lesson { lesson, assignments }` を返し、slide PDF は best-effort fetch (失敗は warn + skip) |
| `--no-fetch-slides` + lesson/page URL | slide PDF fetch をスキップ。`fetchStatus` 省略 |
| `--no-cache` + lesson/page URL | slide cache を無視して強制再取得 (`--no-fetch-slides` 併用時は無視) |
| `--no-fetch-slides --no-cache` | `--no-fetch-slides` が勝つ |
| `--lang ja|en` + lesson/page URL | assignment detail の言語選択 |
| `--lang` + course/courses URL | 受理されるが実質無意味 |
| `--no-fetch-slides` / `--no-cache` + course/courses URL | 受理されるが実質無意味 |

`open` は URL 内 year だけを見るので `--year` は無意味。`lesson show` と同じ
best-effort 分類 (`ok` / `skipped` / `failed`) が `embeds[*].fetchStatus` に載る。

## 6.11 `reset`

認証情報 / 設定 / cookie / キャッシュ / draft をスコープ指定で一括削除する。
別アカウントでの検証、完全なトラブルシュート、PC 譲渡前の初期化などの
ユースケースで使う。`doctor` が報告するパス一覧と 1:1 で対応する。

構文:

```sh
imoocs reset [--scope <SCOPE>...] [--yes] [--dry-run]
```

組み合わせ:

| 組み合わせ | 結果 |
|---|---|
| 既定 (scope 省略) | `all` と同等。対話 TTY では確認プロンプト、非 TTY では `--yes` 必須 |
| `--scope auth` | keyring credential + `cookies.json` |
| `--scope config` | `config.toml` + `course-drive-folders.toml` |
| `--scope cache` | `cookies.json` + `cache_dir/drive/` + `slides_dir` (config/CLI 指定の実パス) |
| `--scope drafts` | `state_dir/drafts/` 配下 |
| `--scope all` | 上記すべて |
| 複数指定 | `--scope auth --scope cache` / `--scope auth,cache` どちらも可 |
| `--yes` / `-y` | 確認プロンプト skip。非 TTY では必須 |
| `--dry-run` | 消さずに対象だけ列挙して exit 0 |

確認プロンプトのデフォルトは No (`y` を明示的に入力しない限り削除しない)。
対象一覧では存在するものを `✓`、存在しないものを `·` + `(not present)` で表示。

終了コード:

| 状況 | exit |
|---|---|
| 正常削除 (一部 not present を含む) | 0 |
| `--dry-run` | 0 |
| 対話プロンプトで `n` | 3 (Validation) |
| 非 TTY で `--yes` 不足 | 3 (Validation) |
| 削除中にエラー 1 件以上 (keyring backend 障害含む) | 5 (Internal) |

keyring 削除が失敗してもファイル系の削除は継続する (fail-soft)。最終 exit は
エラー 1 件以上で 5。**ただし keyring 削除が失敗した場合に限り `config.toml`
の削除は skip される** — 次回リトライ時に `username` から keyring entry を
特定できるようにするため。backend 復旧後に再度 `reset --scope all --yes` を
叩けば残った credential ごとクリーンに消える。

`auth logout` との違い: `auth logout` は keyring + `cookies.json` のみで
config は残す (`reset --scope auth` と等価)。`reset` はスコープを明示して
config や cache まで掃ける。

**safety note**:

- `--scope` を付けるときは 1 つ以上の値が必須 (`imoocs reset --scope --yes`
  のようなタイポは exit 2 で reject される)。scope を省略すれば `all` と
  同等。
- `reset --scope cache` は `slides.out_dir` に由来する `slides_dir` を消す
  が、**safe root (`<cache_dir>/slides` または `/tmp/imoocs/slides`) 内に
  ない場合は refuse + skip する**。`out_dir` に `~/Documents/slides` など
  共有フォルダを指定していると、そこを丸ごと消されずに済む。skip された
  ディレクトリは手作業で削除する前提。
- `reset` は壊れた `config.toml` (parse error) でも動作する — malformed
  config は default 扱いで通して、`--scope config` なら対象ファイルとして
  そのまま削除する。config 復旧経路として使える。

使用例:

```sh
imoocs reset --dry-run                    # 何が消えるかプレビュー
imoocs reset --scope cache --yes          # 再ログインせず cache だけ掃除
imoocs reset --scope all --yes            # CI / agent 向けフル初期化
imoocs reset --scope auth,drafts          # auth 切り直し + 未 push 提出物を破棄
```

`--format` は `auth *` と同様に text 専用で、この flag を無視する。

## 6.12 `completion`

### `completion generate`

構文:

```sh
imoocs completion generate <bash|zsh|fish>
```

組み合わせ:

| shell | 出力先 |
|---|---|
| `bash` | bash completion script を stdout |
| `zsh` | zsh completion script を stdout |
| `fish` | fish completion script を stdout |

実装上の注意:

- Broken pipe は静かに exit 0 にする
- `--format` は無視

### `completion install`

構文:

```sh
imoocs completion install [--shell <bash|zsh|fish>] [--force]
```

shell 解決順位:

1. `--shell`
2. `$SHELL`

組み合わせマトリクス:

| 組み合わせ | 結果 |
|---|---|
| `--shell bash|zsh|fish` | 明示 shell を使う |
| `--shell` なし + `$SHELL` が対応 shell | 自動検出 |
| `--shell` なし + `$SHELL` 未設定 | Validation error |
| `--shell` なし + `$SHELL=/bin/sh` など | Validation error |
| 既存ファイルが同一内容 | 「already up to date」 |
| 既存ファイルが異なる内容 + `--force` なし | Validation error |
| 既存ファイルが異なる内容 + `--force` | 上書き |

配置先:

| shell | path |
|---|---|
| fish | `config_dir/fish/completions/imoocs.fish` |
| bash | `data_dir/bash-completion/completions/imoocs` |
| zsh | `data_dir/zsh/site-functions/_imoocs` |

zsh はインストール後に `fpath` 追記と `~/.zcompdump*` 削除の案内が stderr に出る。

## 7. 実務上の早見表

### 7.1 「どこで設定されるか」

| 対象 | 優先順位 |
|---|---|
| username | CLI `--username` > env `IMOOCS_USERNAME` > `config.username` > prompt |
| password | `--password-stdin` > keyring > prompt |
| slides 出力先 | `slide fetch --out-dir` > `config.slides.out_dir` > `"tmp"` |
| assignment confirm | `config.assignment.confirm` のみ |
| completion install shell | `--shell` > `$SHELL` |
| year | `--year` / `IMOOCS_YEAR` > latest redirect。ただし URL 指定系では URL が勝つ |

### 7.2 「付けても意味がないことが多い組み合わせ」

- `--no-progress`: 現実装では実質 no-op
- `lesson show --no-assignments --lang en`: `--no-assignments` 時は `--lang` 効果なし
- `lesson show --no-fetch-slides --no-cache`: `--no-fetch-slides` が勝ち `--no-cache` は無視
- `open --no-cache` を course URL / courses URL に付ける: slide fetch しない系なので効果なし
- `open --no-fetch-slides` を course URL / courses URL に付ける: 受理されるが効果なし
- `--year` を `open`, `drive *`, `slide fetch`, `auth *`, `completion *` に付ける: 実質意味なし
- `--format json` を `auth *`, `completion *` に付ける: 無視される

### 7.3 「CLI が受理しない組み合わせ」

- `lesson show <course> <lesson> --url <url>`
- `lesson show --url <url> --page <page>`
- `assignment show|submit|upload|push|drafts show --url <url> <course> <problem>`
- `assignment drafts clear --all --url <url>`
- `assignment drafts clear --all <course> <problem>`

### 7.4 「表面仕様と実装の差分」

- `assignment ... --url` は「ページ内 assignment がちょうど 1 個」のときだけ成功
- 複数 assignment のエラーは `--problem-id` を案内するが、その flag は実際には存在しない
- `auth *` は text 専用
- `completion *` は text 専用
- `setup` 失敗時は text mode でも JSON failure envelope
- `--quiet` は direct stderr 出力を抑制しない
- `slide fetch --dump-svgs` は hidden option
