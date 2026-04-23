# iniad-moocs-cli (`imoocs`)

AI agent (Claude Code など) から使うことを前提に作った [INIAD MOOCs](https://moocs.iniad.org/) の**非公式** CLI。
実装は**お察し**のため、**自己責任**でお使いください。

## Quick start

1. **CLI をインストール**
   ```sh
   cargo install --git https://github.com/rarandeyo/iniad-moocs-cli imoocs-cli
   ```

2. **MOOCs / Google SSO にログイン**
   ```sh
   imoocs setup
   ```
   INIAD の username/password を対話で入力すると、以降 `imoocs` が自動でログイン状態を保つ (password は OS のキーチェーンに保存)。
   途中で提出モードを `confirm` (TTY で `y` 確認・それ以外は中断) / `auto` (即確定) から選ぶ。詳細は [Config](#config) 参照。

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
(保存先:`$XDG_CONFIG_HOME/imoocs/course-drive-folders.toml`)。

## Config

`$XDG_CONFIG_HOME/imoocs/config.toml` に保存される。`imoocs setup` で一部項目は対話設定されるが、手で編集してもよい。

| key | 値 | デフォルト | 用途 |
|---|---|---|---|
| `[slides] out_dir` | `"cache"` / `"tmp"` / 絶対パス | `"tmp"` | `imoocs slide fetch` / `imoocs lesson show --fetch-slides` の PDF 保存先。`"cache"` は `$XDG_CACHE_HOME/imoocs/slides/`、`"tmp"` は `/tmp/imoocs/slides/` (OS が自動クリーンアップ)。 |
| `[assignment] confirm` | `"auto"` / `"confirm"` | 未設定 (エラー) | `imoocs assignment submit` / `imoocs assignment upload --force` の確定挙動。下表参照。 |

### `[assignment] confirm` の挙動

| mode | 提出時の挙動 |
|---|---|
| 未設定 | Validation エラーで停止 (`imoocs setup` で選ぶか config を直接編集してください) |
| `auto` | 即**確定** (AI agent に提出を任せる) |
| `confirm` | TTY で `y` を押したときだけ**確定**。それ以外 (拒否 / 非対話 / EOF) は draft 保存せず中断 |

例:

```toml
[slides]
out_dir = "cache"

[assignment]
confirm = "auto"
```

## Commands

```
imoocs setup [--username <u>] [--password-stdin] [--skip-google]
imoocs auth {login,login-google,logout,status,export}
imoocs course {list,show}
imoocs lesson show <courseId> <lessonId> [--page <p>] [--fetch-slides] [--with-assignments]
imoocs slide fetch <embedUrl>
imoocs assignment {list,show,answer,submit,upload}  # --url <url>, --lesson, --status 対応
imoocs drive {list,fetch}                           # INIAD Workspace SAML cookie で
imoocs open <url>                                   # URL 1 本でルーティング
imoocs {doctor,completion,version}
```

All commands output a stable JSON envelope:

```json
{ "success": true, "data": {...} }
{ "success": false, "error": { "code": "...", "message": "...", "hint": "..." } }
```

Exit code: 0 / 1 API / 2 Auth / 3 Validation / 4 NotFound / 5 Internal / 6 Network / 7 NetworkRestricted / 8 NonPublic.

## Docs

- [docs/DESIGN.md](./docs/DESIGN.md) — 設計思想、モジュール構成、
  実装中に踏んだ落とし穴と修正履歴、v2 延期リスト、MOOCs 側 API 早見表。
- [skills/imoocs/SKILL.md](./skills/imoocs/SKILL.md) — agent 向け判断フロー。
- [skills/imoocs/reference/submit-workflow.md](./skills/imoocs/reference/submit-workflow.md) — 課題提出チェックリスト。
- [skills/imoocs/reference/troubleshooting.md](./skills/imoocs/reference/troubleshooting.md) — exit code / 認証切れ対処。
- [skills/imoocs/reference/schema.md](./skills/imoocs/reference/schema.md) — envelope + ドメイン型サンプル。

## License

MIT. Includes adapted code from [moocs-collect](https://github.com/yu7400ki/moocs-collect) (MIT, Copyright 2024 Yuki Natori). See [LICENSE-THIRD-PARTY.md](./LICENSE-THIRD-PARTY.md).
