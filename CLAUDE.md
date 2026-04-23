# CLAUDE.md — iniad-moocs-cli

Claude Code がこのリポジトリで作業する際の前提。詳しい設計は
[docs/DESIGN.md](./docs/DESIGN.md)、エージェント向け操作手順は
[skills/imoocs/SKILL.md](./skills/imoocs/SKILL.md) を読む。

- Rust toolchain は `mise.toml` で 1.93.1 に固定 (`mise install` で取得)。

## 学内ネットワーク制限について

MOOCs の `status=network` / `NETWORK_RESTRICTED` (exit 7) は学内 IP 限定。
該当するのは **出席確認課題と一部のみ** で、大半の課題は学外からも
アクセスできる。学外でこのエラーが出た場合は学内で再実行するようユーザに案内する。他の課題には影響しない。
