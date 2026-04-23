# iniad-moocs-cli (`imoocs`)

ターミナルや AI agent から使うことを想定した [INIAD MOOCs](https://moocs.iniad.org/) の**非公式** CLI。
コース・レッスン・課題情報・スライド・Drive 配布物の閲覧、取得、整理を支援する。
`imoocs assignment submit` / `upload` による提出操作もできるが、提出物の内容、提出判断、提出操作、および関連規約の順守は利用者の責任。

## Quick start

1. **CLI をインストール** — 下記いずれかの方法で。

   **npm (Node 環境を持っている人):**
   ```sh
   npm install -g @rarandeyo/iniad-moocs-cli
   # or: npx @rarandeyo/iniad-moocs-cli --help
   ```

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

   > 備考: `imoocs setup` はパスワードを OS の keyring に保存する。Linux は D-Bus secret-service
   > (gnome-keyring / KeePassXC の secret-service 機能など) が動作している必要がある。

2. **MOOCs / Google SSO にログイン**
   ```sh
   imoocs setup
   ```
   以下の 4 step が順に走る:

   1. **INIAD MOOCs ログイン** — username / password を対話入力 (password は OS のキーチェーンに保存)
   2. **Google SSO セッション取得** — 自動
   3. **提出モード** (`assignment.confirm`) — `confirm` (TTY で `y` 確認・それ以外は中断) / `auto` (即確定) の 2 択。詳細は [Config](#config)
   4. **shell 補完の自動配置** — XDG 標準パスに配置するか確認

3. **2つのAgent skillをinstall**
   ```sh
   gh skill install rarandeyo/iniad-moocs-cli imoocs
   ```

   ```sh
   gh skill install rarandeyo/iniad-moocs-cli imoocs-drive-setup
   ```

4. **履修コースと Drive フォルダを紐付け**
   ```
   /imoocs-drive-setup
   ```
   Agent 上でこの slash command を実行すると、履修中のコースごとに授業資料のDrive フォルダを対話で登録する 
(保存先:`$XDG_CONFIG_HOME/imoocs/course-drive-folders.toml`)。root は Drive 上の
`[受講生]講義資料` フォルダ名から自動発見する。

5. **仕上げ: 完了確認**
   ```sh
   imoocs doctor
   ```
   認証・設定・completion・skill・Drive フォルダを一括検査する。最後の行が
   `Quick start: ✓ 全項目クリア` になれば Quick start 完了。⚠ が残っていれば該当 step に戻る。
   JSON envelope の `quickStartComplete: true` も同じ判定に使える。

## Config

`$XDG_CONFIG_HOME/imoocs/config.toml` に保存される。`imoocs setup` で一部項目は対話設定されるが、手で編集してもよい。

| key | 値 | デフォルト | 用途 |
|---|---|---|---|
| `[slides] out_dir` | `"cache"` / `"tmp"` / 絶対パス | `"tmp"` | `imoocs slide fetch` / `imoocs lesson show --fetch-slides` の PDF 保存先。`"cache"` は `$XDG_CACHE_HOME/imoocs/slides/`、`"tmp"` は `/tmp/imoocs/slides/` (OS が自動クリーンアップ)。 |
| `[assignment] confirm` | `"auto"` / `"confirm"` | 未設定 (エラー) | `imoocs assignment submit` / `imoocs assignment upload` の確定挙動。下表参照。 |

### `[assignment] confirm` の挙動

`submit` / `upload` は常に「確定」を意図するコマンドで、下書き保存専用の verb は存在しない。`assignment.confirm` でゲートの強さを切り替える:

| mode | 提出時の挙動 |
|---|---|
| 未設定 | Validation エラーで停止 (`imoocs setup` で選ぶか config を直接編集してください) |
| `auto` | 確認なしで即**確定** |
| `confirm` | TTY で `y` を押したときだけ**確定**。それ以外 (拒否 / 非対話 / EOF) は API を呼ばずに中断しサーバ状態は変化しない |

例:

```toml
[slides]
out_dir = "cache"

[assignment]
confirm = "auto"
```

## Commands

```
imoocs setup [--username <u>] [--password-stdin] [--skip-google] [--install-completion]
imoocs auth {login,login-google,logout,status,export}
imoocs course {list,show}
imoocs lesson show <courseId> <lessonId> [--page <p>] [--fetch-slides] [--with-assignments]
imoocs slide fetch <embedUrl>
imoocs assignment {list,show,submit,upload}         # --url <url>, --lesson, --status 対応 (submit/upload は常に確定)
imoocs drive {list,search,fetch,folders}            # search/list/fetch は SAML cookie で Drive、folders は course-drive-folders.toml を表示
imoocs open <url>                                   # URL 1 本でルーティング
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

Rust toolchain は `mise.toml` で `1.93.1` に固定。

```sh
mise trust . && mise install       # 初回のみ
```

```sh
cargo build --workspace
```

mise を使わない場合は `rustup` が `Cargo.toml` の `rust-version = 1.93` を見て揃える。
Linux でビルド時に `dbus-1` が見つからないエラーが出たら `libdbus-1-dev` と `pkg-config` を OS のパッケージマネージャで入れる。

## Docs

- [skills/imoocs/SKILL.md](./skills/imoocs/SKILL.md) — agent 向け判断フロー。閲覧・取得・整理を主軸にし、書き込みは明示指示が前提。
- [skills/imoocs/reference/submit-workflow.md](./skills/imoocs/reference/submit-workflow.md) — 課題提出チェックリスト。提出物の内容と提出操作の責任は利用者にある。
- [skills/imoocs/reference/troubleshooting.md](./skills/imoocs/reference/troubleshooting.md) — exit code / 認証切れ対処。
- [skills/imoocs/reference/schema.md](./skills/imoocs/reference/schema.md) — envelope + ドメイン型サンプル。

## License

MIT. Includes adapted code from [moocs-collect](https://github.com/yu7400ki/moocs-collect) (MIT, Copyright 2024 Yuki Natori). See [LICENSE-THIRD-PARTY.md](./LICENSE-THIRD-PARTY.md).
