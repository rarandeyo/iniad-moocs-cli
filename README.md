# iniad-moocs-cli (`imoocs`)

Unofficial CLI for [INIAD MOOCs](https://moocs.iniad.org/), designed to be driven by AI agents (Claude Code etc.).

## Quick start

```sh
cargo install --git https://github.com/rarandeyo/iniad-moocs-cli imoocs-cli
imoocs setup                            # MOOCs login → Google SSO → doctor
gh skill preview rarandeyo/iniad-moocs-cli imoocs
```

`imoocs setup` は対話で INIAD username / password を聞き、成功すれば
`~/.config/imoocs/config.toml` (username) と OS keyring (password) と
`~/.cache/imoocs/cookies.json` を整える。さらに [3/4] で **提出モード**
(`assignment.confirm`) を選ばせる Select が出る — `confirm` (AI agent では
確定されない安全側) / `auto` (即確定) のどちらか。スライド/Drive が不要なら
`imoocs setup --skip-google`。CI 向けには
`echo "$PW" | imoocs setup -u <user> --password-stdin --skip-google`。

### 提出モードと `--force` の意味

`assignment submit` と `assignment upload --force` は「確定を希望する」
意思表示で、実際にサーバに送る `force` は config で決まる:

```toml
[assignment]
confirm = "auto"     # "auto" | "confirm"
```

| mode | submit / upload --force の挙動 |
|---|---|
| 未設定 | exit 3 (`imoocs setup` で選ぶか config を直接編集) |
| `auto` | 常に `force=true` (AI agent を信頼) |
| `confirm` | TTY で `y` を押したときだけ `force=true`。非 TTY 経路は常に `force=false` (下書き保存) |

以前の `-y`/`--yes` フラグは廃止しました。同等の挙動が欲しい場合は
`confirm = "auto"` を設定してください。

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

Exit code: 0 / 1 API / 2 Auth / 3 Validation / 4 NotFound / 5 Internal / 6 Network / 7 NetworkRestricted.

## Agent Skill

配信は [GitHub CLI の `gh skill`](https://cli.github.com/manual/gh_skill_install)
(v2.90+) を公式経路とする。Agent Skills は agentskills.io の open standard 化に
伴い Claude Code / Cursor / GitHub Copilot / Codex / Gemini CLI / Antigravity
が**同一の `SKILL.md` を共有**するので、1 回 install すれば各 agent から
読まれる。

```sh
gh skill install rarandeyo/iniad-moocs-cli　imoocs
```

## Docs

- [docs/DESIGN.md](./docs/DESIGN.md) — 設計思想、モジュール構成、
  実装中に踏んだ落とし穴と修正履歴、v2 延期リスト、MOOCs 側 API 早見表。
- [skills/imoocs/SKILL.md](./skills/imoocs/SKILL.md) — agent 向け判断フロー。
- [skills/imoocs/reference/submit-workflow.md](./skills/imoocs/reference/submit-workflow.md) — 課題提出チェックリスト。
- [skills/imoocs/reference/troubleshooting.md](./skills/imoocs/reference/troubleshooting.md) — exit code / 認証切れ対処。
- [skills/imoocs/reference/schema.md](./skills/imoocs/reference/schema.md) — envelope + ドメイン型サンプル。

## License

MIT. Includes adapted code from [moocs-collect](https://github.com/yu7400ki/moocs-collect) (MIT, Copyright 2024 Yuki Natori). See [LICENSE-THIRD-PARTY.md](./LICENSE-THIRD-PARTY.md).
