# iniad-moocs-cli (`imoocs`)

ターミナルや AI agent から使うことを想定した [INIAD MOOCs](https://moocs.iniad.org/) の**非公式** CLI

コース・レッスン・課題情報・スライド・Drive 配布物の閲覧、取得、整理を支援

`imoocs assignment submit` / `upload` による提出操作もできるが、提出物の内容、提出判断、提出操作等すべての責任は利用者にあります

実装の性質上、予告なく動かなくなる可能性があります。利用は自己責任でお願いします

## 主な機能

- **閲覧** — コース・レッスン・課題・スライド・Drive 配布物を CLI で表示
- **取得** — スライド PDF や Drive ファイルをローカルにダウンロード
- **提出** — 課題の submit / upload。`confirm` モードなら人間が `push` で最終確定
- **URL ルーティング** — `imoocs open <url>` で URL を適切なコマンドに振り分け
- **AI agent 対応** — 全コマンドが JSON envelope + 固定 exit code を返し、agent からも上記すべての操作を実行可能 (agent skill 同梱)

## Quick start

1. **CLI をインストール**

   **推奨: npm** (要 Node.js、クロスプラットフォーム)
   ```sh
   npm install -g @rarandeyo/iniad-moocs-cli
   ```

   <details>
   <summary>Node.js を入れたくない場合の代替</summary>

   **Linux / macOS (shell installer):**
   ```sh
   curl --proto '=https' --tlsv1.2 -LsSf \
     https://github.com/rarandeyo/iniad-moocs-cli/releases/latest/download/imoocs-cli-installer.sh | sh
   ```

   **Windows (PowerShell):**
   ```powershell
   irm https://github.com/rarandeyo/iniad-moocs-cli/releases/latest/download/imoocs-cli-installer.ps1 | iex
   ```

   **ソースから (Rust toolchain 必須):**
   ```sh
   cargo install --git https://github.com/rarandeyo/iniad-moocs-cli imoocs-cli
   ```
   </details>

   **agent-browser もインストール** (課題提出 / Drive / スライド取得に必須)

   `imoocs` はブラウザ操作を [agent-browser](https://github.com/vercel-labs/agent-browser)
   (ヘッドレス Chrome) に委譲する。課題の閲覧だけなら無くても動くが、
   `submit` / `upload` / `push` / `drive` / `slide` 系には必須。
   ```sh
   npm install -g agent-browser   # または: cargo install agent-browser --locked
   ```

2. **MOOCs / Google SSO にログイン**
   ```sh
   imoocs setup
   ```
   > 備考: パスワードは OS の keyring に保存する。Linux は D-Bus secret-service
   > (gnome-keyring / KeePassXC の secret-service 機能など) が動作している必要がある。

   以下の 4 step が順に走る:

   - **INIAD MOOCs ログイン** — username / password を対話入力
   - **Google SSO セッション取得** — 自動
   - **提出モード** (`assignment.confirm`) — 答案提出 (`submit`) / ファイル提出 (`upload`) の挙動を `confirm` / `auto` から選ぶ (詳細は [Config](#config))
     - `confirm` — ローカル draft に stage するだけ。サーバ確定は TTY で `imoocs assignment push` を叩いたとき (AI agent の誤操作対策)
     - `auto` — `submit` / `upload` で即サーバ確定
   - **shell 補完の自動配置** — XDG 標準パスに配置するか確認

3. **Agent skill をインストール** (要 [GitHub CLI (`gh`)](https://github.com/cli/cli))

   AI agent (Claude Code など) から `imoocs` を使うための agent skill を 2 つ入れる。

   ```sh
   gh skill install rarandeyo/iniad-moocs-cli imoocs
   ```

   ```sh
   gh skill install rarandeyo/iniad-moocs-cli imoocs-drive-setup
   ```

4. **履修コースと Drive フォルダを紐付け** (AI agent 内で agent skill として起動)

   AI agent (Claude Code など) の対話プロンプトで以下を実行:
   ```
   /imoocs-drive-setup
   ```
   skill が起動し、履修中のコースごとに授業資料の Drive フォルダを
   対話で登録する (保存先:`$XDG_CONFIG_HOME/imoocs/course-drive-folders.toml`)。root は
   Drive 上の `[受講生]講義資料` フォルダ名から自動発見する。

5. **セットアップ完了を確認**
   ```sh
   imoocs doctor
   ```
   認証・設定・completion・Drive フォルダを一括検査する。最後の行が
   `Quick start: ✓ 全項目クリア` になれば Quick start 完了。⚠ が残っていれば該当 step に戻る。
   JSON envelope の `quickStartComplete: true` も同じ判定に使える。

## Config

`$XDG_CONFIG_HOME/imoocs/config.toml` に保存される。`imoocs setup` で一部項目は対話設定されるが、手で編集してもよい。

| key | 値 | デフォルト | 用途 |
|---|---|---|---|
| `[slides] out_dir` | `"cache"` / `"tmp"` / 絶対パス | `"tmp"` | `imoocs slide fetch` / `imoocs lesson show` / `imoocs open` (lesson URL) が既定で取得する PDF の保存先。`"cache"` は `$XDG_CACHE_HOME/imoocs/slides/`、`"tmp"` は `/tmp/imoocs/slides/` (OS が自動クリーンアップ)。 |
| `[assignment] confirm` | `"auto"` / `"confirm"` | 未設定 (エラー) | `submit` / `upload` の挙動 (即サーバ確定 or ローカル stage)。下表参照。 |

### `[assignment] confirm` の挙動

`submit` / `upload` と `push` で 2-step 運用を切り替える軸。`submit` / `upload` は
「答案を記録する」verb、`push` は「stage した draft をサーバに確定送信する」verb。

| mode | `submit` / `upload` | `push` |
|---|---|---|
| 未設定 | Validation エラーで停止 (`imoocs setup` で選ぶか config を直接編集してください) | 同左 |
| `auto` | 確認なしで即**サーバ確定**（従来互換） | stage があればサーバ確定、無ければ `NOT_FOUND` |
| `confirm` | **ローカル draft に stage するだけ**（TTY/非 TTY 共通、サーバ未送信）。`$XDG_STATE_HOME/imoocs/drafts/` に保存される | TTY 必須。対話プロンプトで `y` を押したときだけ `put_answers(force=true)` と各 `post_file(force=true)` を順次送信 |

`confirm` モードは AI agent がうっかり `submit` を叩いてもサーバに副作用が出ない
安全装置。ユーザは agent が提示した draft の中身を確認してから、TTY で
`imoocs assignment push` を叩いて確定する 2-step フローになる。

例:

```toml
[slides]
out_dir = "cache"

[assignment]
confirm = "auto"
```

## Commands

完全なコマンド/オプション/config/XDG 状態の説明は
[docs/cli-reference.md](./docs/cli-reference.md) を参照。

```
imoocs setup [--username <u>] [--password-stdin] [--skip-google] [--install-completion]
imoocs auth {login,login-google,logout,status,export}
imoocs course {list,show}
imoocs lesson show <courseId> <lessonId> [--page <p>] [--no-assignments] [--no-fetch-slides] [--no-cache]
imoocs slide fetch <embedUrl>
imoocs assignment {list,show,submit,upload,push,drafts}  # confirm モードでは submit/upload は stage のみ、push で確定 (送信は agent-browser 経由)
imoocs assignment drafts {list,show,clear}               # $XDG_STATE_HOME/imoocs/drafts/ の操作
imoocs drive {list,search,fetch,folders}            # list/search/fetch は agent-browser (Chrome) 経由で Drive、folders は course-drive-folders.toml を表示
imoocs open <url>                                   # URL 1 本でルーティング
imoocs reset [--scope auth|config|cache|drafts|all] [--yes] [--dry-run]  # credential / 設定 / cache / draft を一括削除
imoocs completion {generate,install}                # generate=stdout / install=XDG 標準パスに配置
imoocs {doctor,version}
```

All commands output a stable JSON envelope:

```json
{ "success": true, "data": {...} }
{ "success": false, "error": { "code": "...", "message": "...", "hint": "..." } }
```

Exit code: 0 / 1 API / 2 Auth / 3 Validation / 4 NotFound / 5 Internal / 6 Network / 7 NetworkRestricted / 8 NonPublic.

## Development

Rust toolchain は `rust-toolchain.toml` で `1.93.1` + `rustfmt` + `clippy` に固定。
`rustup` が入っていれば repo に `cd` するだけで自動適用される。

```sh
cargo build --workspace
```

`cargo-dist` など Rust 以外のツールは `mise` で管理。

```sh
mise trust . && mise install       # 初回のみ (cargo-dist を入れる)
```

Linux でビルド時に `dbus-1` が見つからないエラーが出たら `libdbus-1-dev` と `pkg-config` を OS のパッケージマネージャで入れる。

## Docs

- [docs/cli-reference.md](./docs/cli-reference.md) — 全コマンド、全オプション、全設定面、組み合わせ、hidden surface の完全リファレンス。
- [skills/imoocs/SKILL.md](./skills/imoocs/SKILL.md) — agent 向け判断フロー。閲覧・取得・整理を主軸にし、書き込みは明示指示が前提。
- [skills/imoocs/reference/submit-workflow.md](./skills/imoocs/reference/submit-workflow.md) — 課題提出チェックリスト。提出物の内容と提出操作の責任は利用者にある。
- [skills/imoocs/reference/troubleshooting.md](./skills/imoocs/reference/troubleshooting.md) — exit code / 認証切れ対処。
- [skills/imoocs/reference/schema.md](./skills/imoocs/reference/schema.md) — envelope + ドメイン型サンプル。

## License

MIT. Includes adapted code from [moocs-collect](https://github.com/yu7400ki/moocs-collect) (MIT, Copyright 2024 Yuki Natori). See [LICENSE-THIRD-PARTY.md](./LICENSE-THIRD-PARTY.md).
