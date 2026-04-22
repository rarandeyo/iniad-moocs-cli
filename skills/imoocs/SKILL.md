---
name: imoocs
description: INIAD MOOCs (moocs.iniad.org) の授業・課題・スライドを Rust 製 CLI `imoocs` から操作するための手順書。ユーザが MOOCs の URL を貼ったとき、「未提出の課題を教えて」「この課題を出したい」「授業資料を読みたい」「スライドを PDF で」と言ったとき、あるいは iniad / 東洋 / moocs / 履修 / 提出 / assignment / submit / 出席 / 採点 などの語が出たときは必ずこの skill を使うこと。URL を明示されていなくても、文脈から INIAD の学習管理システムが対象だと判断できるなら同じく使う。Web ブラウザや Playwright を直接触る必要はない — CLI が認証もスクレイプも肩代わりする。
---

# imoocs — INIAD MOOCs 操作 skill

INIAD MOOCs ([moocs.iniad.org](https://moocs.iniad.org/)) の授業 / 課題 / スライド / Drive 添付を、Rust 製 CLI `imoocs` を介して読み書きするための判断フロー。

## この skill を使う場面

- ユーザが `https://moocs.iniad.org/...` の URL を貼った / 口頭で言及した
- 「未提出の課題を確認したい」「どれが pending か知りたい」
- 「この課題を解いて提出したい」「このレポートを出したい」
- 「授業のスライドが読みたい」「PDF で落として」
- 「INIAD / moocs のログイン通ってる？」「ログインが切れた」
- 単純に MOOCs のコースや授業の内容を要約したい

URL や明示的な MOOCs の単語がなくても、履修 / 課題 / 出席 / 採点 / 提出 / レポートといった語が INIAD 文脈で出たら、まず `imoocs` が使えないか検討する。

## 原則

1. **ブラウザや Playwright を開かない**。前身 MCP (playwright-mcp ベース) とは違い、この CLI は URL 1 本受け取れば内部でスクレイプ / ログイン / PDF 合成まで完結する。agent が DOM を触る必要はない。
2. **`imoocs open <url>` を起点に据える**。URL を渡されたら、パスを手で parse せず `imoocs open <url>` に投げる。返ってきた envelope の `data.type` (`courses` / `course` / `lesson` / `assignment`) で次の一手を決める。
3. **text 出力は人間向け、JSON envelope は agent 向け**。`course` / `lesson` / `assignment` / `slide` / `drive` / `open` は常に JSON。`auth *` は text 専用で exit code で分岐。`doctor` / `setup` は text がデフォルトなので、機械的に処理したいなら `--format json` を付ける。
4. **確定判定は envelope の `submitted` フィールドで行う**。`imoocs assignment submit` が返る envelope に `submitted: true` が載って初めて確定成功。`submitted: false` は「下書きには積まれたが確定されていない」という意味なので、この状態で「提出しました」とユーザに報告してはいけない — 未確定である旨を伝えて判断を仰ぐ。
5. **exit code で分岐**。envelope の `success` も見るが、以下の exit code だけでも大半の分岐が付く:

   | code | 意味 | agent の反応 |
   |------|------|------|
   | 0 | 成功 | そのまま続行 |
   | 1 | `API_ERROR` | MOOCs 側の応答異常。`hint` を読んでリトライか中断 |
   | 2 | `AUTH_EXPIRED` | `imoocs auth login` を案内 / 実行 |
   | 3 | `VALIDATION_ERROR` | 引数や設定の不備。`error.hint` を読む。初回セットアップが未了なら `imoocs setup` を案内 |
   | 4 | `NOT_FOUND` | URL / ID の誤り。ユーザに再確認 |
   | 5 | `INTERNAL_ERROR` | バグ報告を勧める |
   | 6 | `NETWORK_ERROR` | 通信障害。時間を置いて再実行 |
   | 7 | `NETWORK_RESTRICTED` | 学内 IP 限定 (出席確認など一部のみ)。**学内 / VPN で再実行**するようユーザに案内し、他の課題処理は継続できる |

## 最初にやること — 前提チェック

作業開始前に 1 度だけ確認する。ユーザが「ログインしてる？」と明示的に聞いた場合もここに戻る。

1. `imoocs --version` が通ることを確認 (PATH に入っていない場合はインストールを案内)。
2. `imoocs auth status` を叩く。exit 0 なら MOOCs 認証済、exit 2 なら未ログインなので `imoocs setup` (初回) か `imoocs auth login` (再ログイン) を案内する。
3. 詳細が要るときだけ `imoocs doctor --format json` を叩き、認証状態や設定を読む。毎回叩かなくてよい。
4. スライド PDF が必要 / Drive 添付を触る場合のみ、Google 側の認証も確認する (`imoocs auth login-google`)。

初回セットアップ系の `VALIDATION_ERROR` で止まったら、`imoocs setup` を走らせるのが最短。

## ユーザ入力の仕分け

| 入力の形 | 最初に叩くコマンド |
|---|---|
| `https://moocs.iniad.org/courses` | `imoocs course list` |
| `https://moocs.iniad.org/courses/<year>/<course>` | `imoocs open <url>` → `course` 応答 |
| `https://moocs.iniad.org/courses/<year>/<course>/<lesson>[/<page>]` | `imoocs open <url>` → `lesson` 応答 (自動で assignments 展開) |
| 課題ページの URL | `imoocs open <url>` → `assignment` 応答 |
| コース名や講義名しかない | `imoocs course list` で候補を絞り、必要なら `imoocs course show <courseId>` |
| コース ID + 課題だけ知りたい | `imoocs assignment list <courseId> --status pending` |

**URL のパスを agent 側で手でパースしない**。MOOCs は URL のゆらぎ (末尾スラッシュ、ページ ID の有無など) がある。`open` に渡すのが一番安全。

### URL に課題があるはずなのに `open` で見つからないとき

ユーザが「このページの課題を出したい」と言って貼った URL でも、`imoocs open <url>` の応答 (`type: "lesson"`) で `assignments[]` が空 / embeds だけ、ということがある。これは MOOCs 側のレッスンページに課題 HTML が露出していないだけで、**課題自体は同コース内に別経路で存在する**ケースがほとんど。

この場合は次の順で追跡する (他のレッスンに勝手に飛び移らない):

1. URL をそのまま `imoocs open <url>` に再投入して返る envelope の `lesson.lessonId` / `lesson.pageId` をメモ。
2. `imoocs assignment list <courseId> --status pending` (`--status open` でもよい) を叩き、同じ `lessonId` + `pageId` を持つ `AssignmentSummary` を探す。
3. 一致する `problemId` が見つかったら `imoocs assignment show <courseId> <problemId>` で詳細確認。
4. 見つからない場合だけ、ユーザに「このページに紐づく課題が CLI からは取れていません。正しい課題 URL / ID を教えてもらえますか」と確認する。他のレッスン (`DS-00` など) の pending を勝手に「これが提出先」と提示しない。

### 「今日の」「最新回の」と言われたとき

MOOCs の API はレッスンごとの開講日 / 講義スケジュールを返さない。`course show` / `lesson show` の envelope に `scheduledAt` のような日付フィールドは無い。なので「今日の授業」「今週の」「最新回の」を CLI だけで厳密に特定することはできない。

こう振る舞う:

1. `imoocs course show <courseId>` で `lessons[]` を取得。`course.lessons[]` は講義ツリー上の並び順。
2. 連番 (例: `AI-01` → `AI-02` → `AI-03`) の最終回を候補に据える。
3. その lessonId を `imoocs lesson show` で開き、`embeds[]` や `assignments[]` が埋まっているか確認。空なら資料が未掲載 = まだ行われていない回の可能性が高いので、一つ前の回にフォールバックする。
4. どれが「今日の」かに自信が持てない時点で、ユーザに lessonId と候補を提示して確認を取る。**全レッスンを無差別に `lesson show` で舐めない** — 候補は 2–3 件で十分。

## 典型フロー

### A. 未提出課題の棚卸し

1. コース ID が分かっているなら `imoocs assignment list <courseId> --status pending`。分からないなら先に `imoocs course list`。
2. 返ってきた `AssignmentSummary[]` をユーザに提示。`derivedStatus` の意味は `reference/schema.md` 参照:
   - `pending` — open かつ未入力
   - `submitted` — open かつ全 pid 埋まっている
   - `network` — 学内限定 (出席確認系)
   - `closed` / `graded` / `nonpublic` はそのまま表示
3. 個別に中身が知りたいと言われたら `imoocs assignment show <courseId> <problemId>` で掘り下げる。

### B. 課題提出

`reference/submit-workflow.md` にチェックリストがあるので、提出系は必ず目を通す。骨子だけここに置く:

1. **課題を取得**: URL があれば `imoocs open <url>`、ID で分かっているなら `imoocs assignment show <courseId> <problemId>`。返ってきた `fields[]` を読んで、何を埋めれば良いか (textarea / text / radio / checkbox / file) を把握する。
2. **既存の currentValue を尊重**: 既に下書きがあるなら上書きする理由を明示。消さないほうが無難。
3. **ipynb を提出するなら agent 側の準備が要る**:
   - 全セルに実行結果があるか確認。無ければ再実行 (前身 MCP の実運用知)。
   - フォームが html を要求していたら `jupyter nbconvert --to html <path>.ipynb` を agent が実行し、成果物を upload に渡す。
   - 課題文 (markdown) と ipynb の実装 / 出力が整合しているか確認。ズレていたら修正 → 再実行 → 再確認。
4. **下書き保存**: まず `imoocs assignment answer <courseId> <problemId> --data '<json>'` で draft に積む。`--data` は `@path` / `-` (stdin) も使える。`ok: true, submitted: false` が返れば draft 成功。
5. **ファイル添付**: `imoocs assignment upload <courseId> <problemId> --pid <pid> <path>`。`--force` なし = 下書きに保存するだけ。
6. **確定**: ユーザに明示的に「提出して」と言われたら `imoocs assignment submit`。返ってきた envelope の `submitted` フィールドを確認する — `true` なら確定成功、`false` なら「下書きには積まれたが確定されていない」状態なので、その旨をユーザに報告して判断を仰ぐ (勝手に再試行したり「提出しました」と言ったりしない)。stderr に notice が出ている場合はそれも併せて伝える。
7. **成否確認**: 確定が成功したら `imoocs assignment show` を再度叩き、`status` と `fields[*].currentValue` が埋まっていることを確認してからユーザに報告する。
8. **棚卸し**: 今回の課題以外にも同じコースに pending があれば `imoocs assignment list <courseId> --status pending` で一覧にして報告する (前身 MCP の「未提出課題レポート」の習慣)。
9. **書き込みの実施状況を明示する**: 最終応答では「`answer` / `upload` / `submit` のうちどれを叩いたか / 叩いていないか」を必ず1行で伝える。read-only だけで終えた確認タスクなら「下書きもまだ空です。書き込み系は一切叩いていません」等、ユーザが現在の MOOCs 側の状態を誤解しないよう明示する。

### C. レッスン閲覧 / スライド PDF

1. URL があれば `imoocs open <url>`。レッスンページなら `type: "lesson"` が返り、`markdown` 本文 + `embeds[]` + `assignments[]` が同梱される。
2. スライドを PDF で欲しいと言われたら `imoocs lesson show <courseId> <lessonId> --fetch-slides` か、単発で `imoocs slide fetch <embedUrl>` を叩く。保存先は config / `--out-dir` で `/tmp/imoocs/slides/` (default) / `cache` / 絶対パスから選べる。
3. PDF パスは `embeds[*].localPdfPath` に載る。必要なら Read tool で開いて読める (Linux なら `poppler-utils` が要る; 大きい場合は `pages` 指定で分割読み)。
4. **スライドだけで要約が作れないときは Drive 添付も見る**。INIAD では本編コードや配布資料を Google Drive の zip (`ai-s02.zip` など) で配る運用があり、スライド PDF 側は「JupyterLab を起動して〜」の受講準備だけ、ということがある。そのときは同レッスンの `embeds[]` に `type: "google-drive"` のエントリがあるか確認し、あれば `imoocs drive fetch <fileId>` で落として展開してから内容を要約する。スライドの見た目が薄いのを「資料未公開」と早合点しない。
5. Drive ネイティブ形式 (Docs/Sheets/Slides) は現状 API 経由で落とせないので、UI リンクをユーザに案内する。

## 落とし穴 (必ず守ること)

- **`NETWORK_RESTRICTED` (exit 7) は出席確認など一部のみ**。学外でこのエラーが出たら「学内 IP から再実行してください」とユーザに伝え、該当課題以外は普通に進める。全コースが学内限定ではない。
- **`submit` の結果は envelope の `submitted` で判定**。`true` だけが確定成功。`false` なら下書き止まりなので、ユーザに事実を伝えて判断を仰ぐ。勝手に「提出しました」と報告しない。
- **`imoocs auth *` は `--format json` が効かない**。text 出力と exit code で分岐する設計なので、パースを試みない。
- **スライド PDF は `/tmp` が既定**。永続キャッシュではない。「さっきの PDF をもう一度」のときは `--no-cache` 付きで再取得するか、config を `cache` に変えてもらう。

## 実行前に出力モードを意識する

- `course` / `lesson` / `assignment` / `slide` / `drive` / `open` は **JSON envelope 固定**。そのまま parse する。
- `doctor` / `setup` は人間向け。自分で解析するなら `--format json`。
- `auth *` は text + exit code。envelope を期待して読まない。
- グローバル `IMOOCS_FORMAT=json` / `IMOOCS_QUIET=1` / `IMOOCS_NO_PROGRESS=1` は CI や agent から便利。ただし `auth *` には効かない。

## reference

- [`reference/schema.md`](./reference/schema.md) — envelope と主要データ型 (`Course` / `CourseDetail` / `LessonContent` / `AssignmentSummary` / `AssignmentDetail` / `OpenResult` / `Drive*`)。フィールド名と `AssignmentStatus` vs `DerivedStatus` の対応表はここ。
- [`reference/submit-workflow.md`](./reference/submit-workflow.md) — 課題提出のチェックリスト (ipynb 再実行 / html 生成 / 整合性確認 / 事後確認 / 未提出棚卸し) をステップ単位で。
- [`reference/troubleshooting.md`](./reference/troubleshooting.md) — exit code ごとの対処、`NETWORK_RESTRICTED` の案内文例、ログイン切れ / Google SSO 切れの復帰手順。
- 設計の背景を知りたい開発者向けには、リポジトリの [`docs/DESIGN.md`](../../docs/DESIGN.md) 第 4 章 (Gotchas) と第 7 章 (MOOCs 側 API 早見表) が有益。agent が通常作業で読む必要はない。
