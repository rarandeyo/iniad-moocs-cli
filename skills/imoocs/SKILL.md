---
name: imoocs
description: >
  INIAD MOOCs (moocs.iniad.org) の科目・授業・課題を読み書きする Rust 製 CLI
  `imoocs` を使うための skill。初期セットアップ (MOOCs ログイン / Google SSO /
  診断) から、科目一覧・授業閲覧・スライド取得・Drive 配布物取得・課題の
  下書き/提出まで通して扱う。ユーザが MOOCs の URL を渡したとき、授業内容を
  知りたがったとき、「INIAD MOOCs のセットアップをして」「ログインしたい」
  「課題を解きたい」「未着手課題を確認したい」などと言ったときに起動する。
  Keywords: MOOCs, INIAD, セットアップ, 初期設定, login, setup, 課題, 授業,
  提出, assignment, submit, lesson, slide, 履修, pending, open URL.
allowed-tools: Bash(imoocs *)
disable-model-invocation: false
---

# imoocs skill — AI agent 向け INIAD MOOCs オペレーション

`imoocs` は INIAD MOOCs (`moocs.iniad.org`) を AI agent が扱える形に薄く
ラップした CLI。

- **agent 向け verb** (`course` / `lesson` / `assignment` / `slide` /
  `drive` / `open` / `version`) は stdout に stable JSON envelope
  `{"success": true, "data": ...}` もしくは
  `{"success": false, "error": {"code", "message", "hint"?, "details"?}}`
  を返す。`--format` に関わらず常に JSON。
- **人間向け verb** (`doctor` / `auth *` / `setup`) は default だと text
  サマリを出す。agent からは **`--format json` (または
  `IMOOCS_FORMAT=json`) を付けて**同じ envelope を取得すること。
- 失敗時は verb 種別に関わらず stdout に JSON envelope (text モードでも)。
- 進捗や警告は stderr (text モードは compact text、JSON モードは tracing JSON)。

---

## 最初にやること

1. `imoocs --version` でバイナリ導入を確認。未導入なら
   `cargo install --git https://github.com/rarandeyo/iniad-moocs-cli imoocs-cli`
   をユーザに案内 (prebuilt / AUR / Homebrew は v2 候補)。
2. `imoocs doctor --format json` で環境確認 (agent 向けには JSON が必須。
   素のままだと text サマリになる)。
3. `moocsAuthenticated: false` または `googleAuthenticated: false` なら
   `imoocs setup` をユーザに走らせてもらう。これは auth login →
   auth login-google → doctor を順次実行するファサード。username と
   password は対話入力なので agent 単独では完了しない。`--format json`
   で呼ぶと `{success, data: {steps: [...], allOk}}` が返るので各 step を
   parse して案内できる。
   - スライド/Drive が不要で学外にいるときは `imoocs setup --skip-google`。
   - CI / 非対話では `echo $PW | imoocs setup -u <user> --password-stdin --skip-google`。
   - setup 中の [3/4] で提出モード (`assignment.confirm`) を選ばせる Select が出る。
     既定は `confirm` (AI agent 経由では確定しない安全側)、agent に任せたい
     環境では `auto` を選ぶ。後から `$XDG_CONFIG_HOME/imoocs/config.toml` で変更可。
4. 認証が済んだ後は以降の判断フローに進む。

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
- **ユーザが「Drive」/「配布ファイル」/「zip をダウンロード」等と言った**、
  または embed に `type: "google-drive"` が含まれる →
  - `kind: "folder"` なら先に `imoocs drive list <url>` で中身を把握
    (最大 50 件、`truncated: true` が返ったら 50 件超で切れている可能性
    あり — 必要ならユーザーにブラウザでの補完を促す)
  - `kind: "file"` または folder 内で目的ファイルが決まったら
    `imoocs drive fetch <url-or-fileId>`。`data.localPath` を Read tool
    で開ける。拡張子は Content-Disposition / mime から自動判定
  - Google ネイティブ型 (Docs/Sheets/Slides) はエラーで返るので対応
    不可をユーザーに伝える (v2 で `--export pdf` 予定)

## コマンドチートシート

| 目的 | コマンド |
|---|---|
| 認証確認 | `imoocs auth status` / `imoocs doctor` |
| ログイン | `imoocs auth login` (MOOCs) / `imoocs auth login-google` |
| コース一覧 | `imoocs course list` |
| コース詳細 (授業一覧) | `imoocs course show <courseId>` → `data.groups` で章立て |
| 授業ページ | `imoocs lesson show <c> <l> [--page <p>]` |
| スライド PDF | `... --fetch-slides [--no-cache]` |
| Drive フォルダ一覧 | `imoocs drive list <folder-url-or-id>` |
| Drive ファイル DL | `imoocs drive fetch <file-url-or-id> [--out <path>]` |
| 課題一覧 | `imoocs assignment list <c> [--lesson <l>] [--status pending\|submitted\|...]` |
| 課題詳細 | `imoocs assignment show <c> <p>` / `--url <url>` |
| 回答下書き | `echo '{"pid":"..."}' \| imoocs assignment answer <c> <p> --data -` |
| 最終提出 | `imoocs assignment submit <c> <p>` (実 `force` は config で決まる) |
| ファイル提出 (下書き) | `imoocs assignment upload <c> <p> --pid <pid> <path>` |
| ファイル提出 (確定) | `imoocs assignment upload <c> <p> --pid <pid> <path> --force` |
| URL 自動ルーティング | `imoocs open <url>` |

agent 向け verb (`course` / `lesson` / `assignment` / `slide` / `drive` /
`open` / `version`) は `--format` に関係なく常に整形 JSON envelope を返す。
人間向け verb (`doctor` / `auth *` / `setup`) のみ default が text 要約で、
`--format json` (または `IMOOCS_FORMAT=json`) で JSON envelope に切替。

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
5. OK が出たら `imoocs assignment submit <c> <p>` を実行
   - config が `confirm = "auto"` なら即 `force=true` で確定
   - config が `confirm = "confirm"` なら TTY に y/N prompt。non-TTY 経由
     (agent 実行) では必ず下書き保存になり、ユーザが手元で再実行する必要あり
   - config `assignment.confirm` 未設定時は exit 3 (明示選択を要求)

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
- `status=network` / `NETWORK_RESTRICTED` (exit 7) は学内 IP 限定。
  該当するのは**出席確認課題と一部のみ**で大半の課題は学外でも可。
  出たら学内 / VPN に繋いでから再実行するよう促す。
- スライド PDF はページ数が多いと Read tool が重くなる。
  `pages: "1-5"` などで分割して読む。
- スライド PDF の保存先はデフォルトで `/tmp/imoocs/slides/` (再起動で消える)。
  恒久的に保持したいユーザには `config.toml [slides] out_dir = "cache"` を
  案内、1 回だけ別の場所に置きたいなら `imoocs slide fetch --out-dir <cache|tmp|PATH>`。
- 認証情報は OS keyring に保存されているので、Bash で
  `--password-stdin` を安易に渡さない (CI / automation 専用)。
  `imoocs auth export` は username と「keyring に保存されているか」だけを
  text で返し、password 本体は CLI から決して出力されない。
