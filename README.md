# iniad-moocs-cli (`imoocs`)

Unofficial CLI for [INIAD MOOCs](https://moocs.iniad.org/), designed to be driven by AI agents (Claude Code etc.).

## Quick start

```sh
cargo install --git https://github.com/rarandeyo/iniad-moocs-cli imoocs-cli
imoocs auth login           # MOOCs Keycloak (username 対話 + keyring)
imoocs auth login-google    # Google SAML (スライド PDF 用)
imoocs doctor
imoocs course list
```

## Commands

```
imoocs auth {login,login-google,logout,status,export}
imoocs course {list,show}
imoocs lesson show <courseId> <lessonId> [--page <p>] [--fetch-slides] [--with-assignments]
imoocs slide fetch <embedUrl>
imoocs assignment {list,show,answer,submit,upload}  # --url <url>, --lesson, --status 対応
imoocs open <url>                                   # URL 1 本でルーティング
imoocs {doctor,completion,skill,version}
```

All commands output a stable JSON envelope:

```json
{ "success": true, "data": {...} }
{ "success": false, "error": { "code": "...", "message": "...", "hint": "..." } }
```

Exit code: 0 / 1 API / 2 Auth / 3 Validation / 4 NotFound / 5 Internal / 6 Network / 7 NetworkRestricted.

## Agent Skill

After `cargo install`, install the Claude Code skill:

```sh
imoocs skill install --user   # → ~/.claude/skills/imoocs/
# or manually:
ln -s "$(pwd)/skills/imoocs" ~/.claude/skills/imoocs
```

## Docs

- [docs/DESIGN.md](./docs/DESIGN.md) — 設計思想、モジュール構成、
  実装中に踏んだ落とし穴と修正履歴、v2 延期リスト、MOOCs 側 API 早見表。
- [skills/imoocs/SKILL.md](./skills/imoocs/SKILL.md) — agent 向け判断フロー。
- [skills/imoocs/reference/commands.md](./skills/imoocs/reference/commands.md) — コマンドリファレンス。
- [skills/imoocs/reference/schema.md](./skills/imoocs/reference/schema.md) — envelope + ドメイン型サンプル。

## License

MIT. Includes adapted code from [moocs-collect](https://github.com/yu7400ki/moocs-collect) (MIT, Copyright 2024 Yuki Natori). See [LICENSE-THIRD-PARTY.md](./LICENSE-THIRD-PARTY.md).
