# 課題提出チェックリスト

`imoocs assignment` 系で課題を出すときの段取り。SKILL.md のフロー B から呼ばれる詳細版。前身 MCP (Playwright) 時代から引き継ぐべき「agent 側の運用知」をここに集約している。

この文書は提出操作を進めるときの作業メモであり、提出物の内容、提出判断、提出操作、および関連規約の順守は利用者の責任。agent は明示的な依頼なしに `submit` / `upload` を実行しない。

## 0. 前提

- MOOCs 認証済 (`imoocs auth status` → exit 0)。
- 初回セットアップ (`imoocs setup`) が済んでいる。
- 対象課題の `courseId` / `problemId` / URL のいずれかが手元にある。

## 1. 課題内容を取得して読む

URL があれば:

```sh
imoocs open <url>
```

ID だけなら:

```sh
imoocs assignment show <courseId> <problemId>
```

envelope の `data.fields[]` を読み、以下を把握する:

- 各 pid がどの型か (`textarea` / `text` / `radio` / `checkbox` / `file`)
- 既に `currentValue` / `uploadedFile` が埋まっている pid があるか (過去の確定提出 or Web UI で残した下書き)
- `lang` (ja/en) と、問題文 / 指示が何を要求しているか

`markdown` (レッスン側の `lesson show`) にノートブックテンプレートやサンプルコードへのリンクが載っていることがあるので、そちらも併読する。

## 2. ipynb を提出するときの前処理

ユーザの指示が `.ipynb` ファイルを添付する形なら、以下を順番に確認する。

### 2a. 全セルに実行結果がある?

- JupyterLab / VS Code で手動実行でも、`jupyter nbconvert --to notebook --execute <path>.ipynb --inplace` でも良い。
- どれか 1 セルでも `execution_count == null` / `outputs == []` のまま残っていれば、再実行する。
- 「このノートを実行して」とユーザが言っているなら、`papermill` や `nbconvert --execute` でまとめて再実行する。

### 2b. 課題文との整合性

課題で求められていること (変数名、出力形式、関数シグネチャ、図の枚数など) と、ノートブックの実装 / 出力を突き合わせる。典型的なずれ:

- 変数名が `x` になっていて、課題は `result` を要求している
- `print` していない (セル末尾の式評価のみ)
- 図が要求されているがセル出力が数値だけ

ずれていたら、コードを修正 → 全セル再実行 → 再度突き合わせ。このループは agent の責務。CLI はやらない。

### 2c. HTML 版が必要か?

提出フォームの `fields[]` に `type: "file"` + `pid: "html"` (あるいは問題文に "html" と明記) があれば html が要る。以下のどちらか:

```sh
jupyter nbconvert --to html <path>.ipynb
# 実行結果を残したいとき:
jupyter nbconvert --to html --execute <path>.ipynb
```

生成された `<path>.html` を upload の対象にする。

## 3. 提出データをローカル JSON で組み立てる

CLI にはサーバ下書きを積む verb は無い。テキスト / ラジオ / チェックボックス系の答えは `{pid: value}` マップをローカルファイルに書いてユーザに内容を見せる:

```sh
# 例: /tmp/draft.json
{
  "p1": "answer text",
  "p2": "choice-a",
  "p3": ["value1", "value2"]
}
```

値の組み立て方:

- `textarea` / `text` → 文字列
- `radio` → `options[*].value` のいずれか
- `checkbox` → `options[*].value` を含む配列

この段階では MOOCs サーバに何も送らない。agent は JSON の中身をユーザに提示し、「これで提出して良いか」を確認する。

## 4. submit (テキスト系) — auto なら確定、confirm なら stage

ユーザが明示的に「出して」「提出して」と言ったときだけ叩く:

```sh
imoocs assignment submit <courseId> <problemId> --data @/tmp/draft.json
```

`--data` は 3 形式:

- `--data '{"p1": "text", "p2": "choice-a"}'` — inline JSON
- `--data @/path/to/file.json` — ファイル
- `--data -` — stdin

`assignment.confirm` 設定によって挙動が切り替わる:

- `auto` → 即サーバ確定 (force=true, envelope は `AnswerResult { submitted: true }`)
- `confirm` → **サーバに送らずローカル `$XDG_STATE_HOME/imoocs/drafts/` に stage** (TTY/非 TTY 共通、envelope は `StagedResult { staged: true, submitted: false, draftPath, hint }`)
- 未設定 → exit 3 (`VALIDATION_ERROR`)。`imoocs setup` を案内

`confirm` モードの submit は agent が安全に叩ける (サーバに触らない)。ユーザに envelope の `draftPath` と `answers` を見せ、`push` の実行を依頼する (§5.5)。

## 5. upload (ファイル) — auto なら確定、confirm なら stage

```sh
imoocs assignment upload <courseId> <problemId> --pid <pid> <path>
```

テキスト系 submit と同じゲート軸。`auto` なら即サーバ確定 (POST /file)、`confirm` なら draft の `files[pid]` に **絶対パス** で記録されるだけ。Content-Type は CLI が `mime_guess` で自動設定する。

`confirm` モードで upload を重ねると、`files[pid]` は pid ごとに独立上書き、`answers` とは干渉しない。複数 pid の成果物を順次積んで、最後に `push` で一括確定できる。

## 5.5. `push` — stage した draft をサーバに確定送信する

`confirm` モードで作った stage は、TTY 必須の `push` コマンドでサーバに送る:

```sh
imoocs assignment push <courseId> <problemId>
# or: imoocs assignment push --url <lesson-page-url>
```

- TTY 必須。agent (非 TTY) から叩くと exit 3 (`VALIDATION_ERROR`)。draft は保持。
- 対話プロンプトで `y` を押すと `put_answers(force=true)` → 各 `post_file(force=true)` を順次送信。
- 全部成功したら draft ファイルを削除し、envelope `PushResult { pushed: true, submitted: true, answersSubmittedPids, filesSubmittedPids, status }` を返す。
- 途中失敗 (サーバ 5xx / ネットワーク断など) は draft を残して exit 1/6。`error.message` に「Draft retained at \<path\>. Re-run \`imoocs assignment push\` to resume.」が入る。**サーバ側は部分確定の可能性あり**（answers は送れて files が一部未送など）。再 `push` は冪等に整合するので、素直に再実行する。

auto モードでも `push` は動く (stage があれば)。stage 無しで叩いたら `NOT_FOUND` (exit 4)。

agent は `push` を叩かない。envelope の `hint` をそのままユーザに伝えて「TTY で実行して」と依頼する。勝手にリトライしない。

## 6. 事後確認

`push` 成功後、`imoocs assignment show <courseId> <problemId>` を叩き直して:

- `status` が `open` / `graded` / `closed` のどれか
- `fields[*].currentValue` / `uploadedFile` が埋まっている
- `derivedStatus` が `submitted` に遷移している (open かつ全 pid 埋まり)

が成立していることを確認してからユーザに報告する。

`auto` モードの submit/upload で直接確定した場合も同じ手順で確認する。

## 7. 未提出棚卸し

前身 MCP から引き継ぐ作法。提出が一段落したら、同じコースの他の pending を確認して報告する:

```sh
imoocs assignment list <courseId> --status pending
```

> 提出完了しました。ついでに同じコースで他に未提出の課題が N 件あります:
> - `assignment-03` (...)
> - `assignment-05` (...)

ユーザに続けて解く意思があるか確認する。

## 8. 失敗時の分岐

| 症状 | 対処 |
|---|---|
| `exit 2` / `AUTH_EXPIRED` | `imoocs auth login` を案内 / 実行して再試行 |
| `exit 3` / `VALIDATION_ERROR` (`push` を非 TTY で実行) | `assignment push` は TTY 必須。agent / パイプから叩くと exit 3 で止まる (API は呼ばれていない、draft 保持)。ユーザに TTY から `imoocs assignment push <c> <p>` を叩くよう依頼する |
| `exit 3` / `VALIDATION_ERROR` (`push` プロンプトで `n` / EOF) | `push` 対話で拒否された。draft はそのまま保持されている。ユーザが準備できたら再 `push` すればよい |
| `exit 3` / `VALIDATION_ERROR` (config 未設定) | `assignment.confirm` 未設定で submit / push を叩いた。`imoocs setup` を案内 |
| `exit 3` / `VALIDATION_ERROR` (その他) | `error.hint` を読む。`--data` の JSON 不備なら `assignment show` で `fields[*].pid` を再確認 |
| `exit 4` / `NOT_FOUND` (`push`) | 該当 draft が `$XDG_STATE_HOME/imoocs/drafts/` に無い。先に `imoocs assignment submit` / `upload` で stage する |
| `exit 4` / `NOT_FOUND` (その他) | URL / problemId を再確認。`course show` → `lesson show` で辿り直す |
| `exit 1` / `API_ERROR` (`push` 途中失敗) | `put_answers` または途中の `post_file` が 5xx で落ちた。draft は保持されているので、しばらく置いて再 `push`。answers は force=true で冪等に再送されるので副作用なし。サーバ側で answers だけ確定している可能性はある (部分確定) |
| `exit 6` / `NETWORK_ERROR` (`push` 途中失敗) | 通信断で push が途切れた。`API_ERROR` と同じく再 `push` で resume する |
| `exit 7` / `NETWORK_RESTRICTED` | 出席確認など学内限定の課題のみ。学内 / VPN で再実行を案内 |
