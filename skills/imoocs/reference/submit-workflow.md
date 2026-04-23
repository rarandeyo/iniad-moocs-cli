# 課題提出チェックリスト

`imoocs assignment` 系で課題を出すときの段取り。SKILL.md のフロー B から呼ばれる詳細版。前身 MCP (Playwright) 時代から引き継ぐべき「agent 側の運用知」をここに集約している。

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
- 既に `currentValue` / `uploadedFile` が埋まっている pid があるか (draft 状態)
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

## 3. 下書き保存 (draft)

テキスト / ラジオ / チェックボックス系は `answer` で下書きに積む:

```sh
imoocs assignment answer <courseId> <problemId> --data @/tmp/draft.json
```

`--data` は 3 形式:

- `--data '{"p1": "text", "p2": "choice-a"}'` — inline JSON
- `--data @/path/to/file.json` — ファイル
- `--data -` — stdin

答えは `{<pid>: <value>}` のマップ。`radio` は `options[*].value`、`checkbox` は `["value1", "value2"]` の配列、`textarea` / `text` は文字列。

返り値 `{ok: true, submitted: false, savedAt: "..."}` が出れば draft 成功。`submitted: false` が正常値 (まだ確定していない)。

## 4. ファイルアップロード (draft)

```sh
imoocs assignment upload <courseId> <problemId> --pid <pid> <path>
```

`--force` を付けなければ確定はしない (draft)。Content-Type は CLI が `mime_guess` で自動設定するので agent は意識しなくて良い。

アップロード後に `imoocs assignment show` を再度叩き、`fields[*].uploadedFile` が non-null になっていることを確認する。

## 5. 確定提出

ユーザが明示的に「出して」「提出して」と言ったときだけ:

```sh
imoocs assignment submit <courseId> <problemId>
```

あるいはファイルも同時に確定したい場合:

```sh
imoocs assignment upload <courseId> <problemId> --pid <pid> <path> --force
```

返ってきた envelope の `submitted` で判定する:

- `submitted: true` → 確定成功。ステップ 6 に進む。
- `submitted: false` → 下書きには積まれたが確定されていない。ユーザにその旨を伝え、どうするか判断を仰ぐ (勝手に再試行したり「提出しました」と報告したりしない)。stderr に notice が出ていれば内容をそのまま引用する。

## 6. 事後確認

確定後、`imoocs assignment show <courseId> <problemId>` を叩き直して:

- `status` が `open` / `graded` / `closed` のどれか
- `fields[*].currentValue` / `uploadedFile` が埋まっている
- `derivedStatus` が `submitted` に遷移している (open かつ全 pid 埋まり)

が成立していることを確認してからユーザに報告する。

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
| `exit 3` / `VALIDATION_ERROR` (confirm モード) | `assignment.confirm = "confirm"` 設定下で `submit` / `upload --force` が非 TTY (agent / パイプ) から呼ばれた、あるいは TTY で `n` が押された。**API は呼ばれていないのでサーバ状態は変わっていない**。ユーザにその旨を伝え、TTY から再実行してもらうか `confirm = "auto"` への切替を提案する |
| `exit 3` / `VALIDATION_ERROR` (その他) | `error.hint` を読む。初回セットアップが未了なら `imoocs setup` を案内。`--data` の JSON 不備なら `assignment show` で `fields[*].pid` を再確認 |
| `exit 4` / `NOT_FOUND` | URL / problemId を再確認。`course show` → `lesson show` で辿り直す |
| `exit 7` / `NETWORK_RESTRICTED` | 出席確認など学内限定の課題のみ。学内 / VPN で再実行を案内 |
| `submitted: false` が返る | 下書きに保存されただけ。envelope と stderr の notice をそのままユーザに伝え、どうするか判断を仰ぐ |
