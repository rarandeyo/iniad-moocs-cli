# iniad-moocs-cli (`imoocs`)

Unofficial CLI for [INIAD MOOCs](https://moocs.iniad.org/), designed to be driven by AI agents (Claude Code etc.).

**Status**: early development (Phase 1)

## Quick start

```sh
cargo install --git https://github.com/rarandeyo/iniad-moocs-cli imoocs-cli
imoocs auth login
imoocs doctor
imoocs course list
```

## Commands (target)

```
imoocs auth {login,login-google,logout,status,export}
imoocs course {list,show}
imoocs lesson show <courseId> <lessonId> [--fetch-slides]
imoocs slide fetch <embedUrl>
imoocs assignment {list,show,answer,submit,upload}
imoocs {doctor,schema,api,completion,generate,skill,version}
```

All commands output a stable JSON envelope:

```json
{ "success": true, "data": {...} }
{ "success": false, "error": { "code": "...", "message": "...", "hint": "..." } }
```

## Agent Skill

After `cargo install`, install the Claude Code skill:

```sh
imoocs skill install --user   # → ~/.claude/skills/imoocs/
# or manually:
ln -s "$(pwd)/skills/imoocs" ~/.claude/skills/imoocs
```

## License

MIT. Includes adapted code from [moocs-collect](https://github.com/yu7400ki/moocs-collect) (MIT, Copyright 2024 Yuki Natori). See [LICENSE-THIRD-PARTY.md](./LICENSE-THIRD-PARTY.md).
