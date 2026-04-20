---
name: imoocs
description: >
  INIAD MOOCs (moocs.iniad.org) の科目・授業・課題を読み書きする Rust 製 CLI
  `imoocs` を使うための skill。ユーザが MOOCs の URL を渡したとき、
  授業内容を知りたがったとき、課題を解きたい/未着手課題を確認したい
  ときに起動する。Keywords: MOOCs, INIAD, 課題, 授業, 提出, assignment,
  submit, lesson, slide, 履修, pending, open URL.
allowed-tools: Bash(imoocs *)
disable-model-invocation: false
---

# imoocs skill — AI agent 向け INIAD MOOCs オペレーション

`imoocs` は INIAD MOOCs (`moocs.iniad.org`) を AI agent が扱える形に薄く
ラップした CLI。全コマンドは stdout に stable JSON envelope
`{"success": true, "data": ...}` もしくは
`{"success": false, "error": {"code", "message", "hint"?, "details"?}}`
を返す。進捗や警告は stderr (tracing JSON)。

---

## 最初にやること

1. `imoocs doctor` を叩いて環境確認。`moocsAuthenticated: false` なら
   `imoocs auth login` をユーザに促す (対話入力なので agent 単独では
   完了しない)
2. スライドを開く必要があると分かっている場合は `imoocs auth login-google`
   も事前に走らせておく (keyring から username/password 流用で対話なし)

## 判断フロー

- **ユーザが MOOCs の URL を渡してきた** →
  `imoocs open <url>` が最も情報量が多い (`OpenResult` enum)。
  lesson URL なら `{lesson, assignments: [AssignmentDetail, ...]}` を
  一度に取れる。
- **ユーザが「この課題」と言った** →
  URL あるなら `imoocs assignment show --url <url>`、
  courseId と problemId が分かっていれば `imoocs assignment show <c> <p>`。
- **ユーザが「未着手課題」と言った** →
  `imoocs assignment list <courseId> --status pending` (未回答のみ)。
  `--lesson <id>` で授業単位に絞れる。
- **ユーザが「スライドを読んで」と言った** →
  `imoocs lesson show <c> <l> --page <p> --fetch-slides` で
  `data.embeds[].localPdfPath` に PDF パスが入る。Claude の `Read` tool
  で開ける。ページ数が多いときは `pages: "1-5"` 等でレンジ分割。

## コマンドチートシート

| 目的 | コマンド |
|---|---|
| 認証確認 | `imoocs auth status` / `imoocs doctor` |
| ログイン | `imoocs auth login` (MOOCs) / `imoocs auth login-google` |
| コース一覧 | `imoocs course list` |
| コース詳細 (授業一覧) | `imoocs course show <courseId>` → `data.groups` で章立て |
| 授業ページ | `imoocs lesson show <c> <l> [--page <p>]` |
| スライド PDF | `... --fetch-slides [--no-cache]` |
| 課題一覧 | `imoocs assignment list <c> [--lesson <l>] [--status pending\|submitted\|...]` |
| 課題詳細 | `imoocs assignment show <c> <p>` / `--url <url>` |
| 回答下書き | `echo '{"pid":"..."}' \| imoocs assignment answer <c> <p> --data -` |
| 最終提出 | `imoocs assignment submit <c> <p> --yes` (force=true) |
| ファイル提出 | `imoocs assignment upload <c> <p> --pid <pid> <path>` |
| URL 自動ルーティング | `imoocs open <url>` |

`--format pretty` は人間向け、デフォルトは機械可読 JSON。

## 回答と提出の標準手順

1. `imoocs assignment show <c> <p>` で `data.fields` を取得。
   各 field は `{type, pid, label, options?, currentValue?, ...}`。
2. 回答を pid→value の JSON で組み立てる:
   ```json
   {"p1": "回答本文", "p2-01": "OK"}
   ```
   - textarea/text: 文字列
   - radio: option の `value`
   - checkbox: カンマ区切り文字列 (サーバ側の仕様に合わせる)
3. stdin で `imoocs assignment answer <c> <p> --data -` → `submitted:false` で下書き保存
4. **ユーザに最終提出して良いか必ず確認**
5. OK が出たら `imoocs assignment submit <c> <p> --yes` (`force=true` = 確定)

`--yes` なしの `submit` は Validation エラーで exit 3 (事故防止)。

## 返り envelope の要点

- すべてのデータキーは camelCase
- 失敗時は `success:false` + exit code:
  - 2 = 認証切れ (`imoocs auth login` を促す)
  - 3 = validation (引数 or JSON 不正)
  - 4 = 見つからない
  - 7 = 学内ネットワーク制限 (`NETWORK_RESTRICTED`)

詳細は `${CLAUDE_SKILL_DIR}/reference/commands.md` と
`${CLAUDE_SKILL_DIR}/reference/schema.md` を **Read tool で読んでから**
続行してください (このファイルからは自動ロードされません)。

## 注意事項

- `submit` は force=true の確定提出なので**必ずユーザ確認後**。
- `status=network` の課題は学内 IP 限定。agent はエラー内容をユーザに
  伝えて待機する。
- スライド PDF はページ数が多いと Read tool が重くなる。
  `pages: "1-5"` などで分割して読む。
- 認証情報は OS keyring に保存されているので、Bash で
  `--password-stdin` や `--unmasked` を安易に渡さない。
