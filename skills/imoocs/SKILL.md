---
name: imoocs
description: INIAD MOOCs (moocs.iniad.org) のコース・レッスン・課題情報・スライド・Drive 配布物の閲覧、取得、整理を Rust 製 CLI `imoocs` から行うための手順書。課題確認や提出操作を扱う場合も、提出物の内容、提出判断、提出操作、および関連規約の順守は利用者の責任とする。ユーザが MOOCs の URL を貼ったとき、「未提出の課題を教えて」「この課題を出したい」「授業資料を読みたい」「スライドを PDF で」と言ったとき、あるいは iniad / 東洋 / moocs / 履修 / 提出 / assignment / submit / 出席 / 採点 などの語が出たときは必ずこの skill を使うこと。URL を明示されていなくても、文脈から INIAD の学習管理システムが対象だと判断できるなら同じく使う。Web ブラウザや Playwright を直接触る必要はない — CLI が認証もスクレイプも肩代わりする。
---

# imoocs — INIAD MOOCs 操作 skill

INIAD MOOCs ([moocs.iniad.org](https://moocs.iniad.org/)) の授業 / 課題 / スライド / Drive 添付を、Rust 製 CLI `imoocs` を介して読み書きするための判断フロー。

この skill は閲覧・取得・整理を主軸に据える。書き込み系 (`submit` / `upload`) を扱う場合でも、実行はユーザの明示指示が前提で、提出物の内容、提出判断、提出操作、および関連規約の順守は利用者の責任とする。

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
3. **text 出力は人間向け、JSON envelope は agent 向け**。`course` / `lesson` / `assignment` / `slide` / `drive` / `open` は常に JSON。`auth *` は text 専用で exit code で分岐。`doctor` / `setup` は text がデフォルトなので、機械的に処理したいなら `--format json` を付ける。実際の config/network 障害では `doctor --format json` も failure envelope で落ちるので、`success: true` を前提にしない。
4. **confirm モードは 2-step (stage → push)**。`assignment.confirm = "confirm"` のとき、`imoocs assignment submit` / `upload` は **サーバに送らずローカル draft に stage するだけ** (HTTP を一切叩かない、非 TTY/TTY 共通)。envelope は `StagedResult { staged: true, submitted: false, draftPath, hint }` で exit 0。確定は TTY 必須の `imoocs assignment push` が担当し、対話プロンプトで `y` を押したときだけ `put_answers(force=true)` と各 `post_file(force=true)` を順次送信する。全部成功で draft 削除、途中失敗で draft 保持 (`API_ERROR`/`NETWORK_ERROR`)。`assignment.confirm = "auto"` の場合は従来通り submit/upload が即サーバ確定し、envelope `AnswerResult { submitted: true }` を返す。`push` を叩く必要はない。`submitted` の値でどの段階まで進んだか判別する: stage 済は `false`、push 済は `true`。
5. **exit code で分岐**。envelope の `success` も見るが、以下の exit code だけでも大半の分岐が付く:

   | code | 意味 | agent の反応 |
   |------|------|------|
   | 0 | 成功 | そのまま続行 |
   | 1 | `API_ERROR` | MOOCs 側の応答異常。`hint` を読んでリトライか中断 |
   | 2 | `AUTH_EXPIRED` | `imoocs auth login` を案内 / 実行 |
   | 3 | `VALIDATION_ERROR` | 引数や設定の不備 / `assignment push` を非 TTY で叩いた / `push` プロンプトで `n`。`error.hint` を読む。初回セットアップが未了なら `imoocs setup`。`confirm` モードの submit/upload は exit 0 で stage されるので、ここには来ない |
   | 4 | `NOT_FOUND` | URL / ID の誤り。ユーザに再確認 |
   | 5 | `INTERNAL_ERROR` | バグ報告を勧める |
   | 6 | `NETWORK_ERROR` | 通信障害。時間を置いて再実行 |
   | 7 | `NETWORK_RESTRICTED` | 学内 IP 限定 (出席確認など一部のみ)。**学内 / VPN で再実行**するようユーザに案内し、他の課題処理は継続できる |
   | 8 | `NON_PUBLIC` | 未公開課題 (`atnd-*` が講義前 / `ai-03-*` が解禁前 等) に `assignment show` を叩いたときの正常応答。agent は「解禁を待つ」か、ユーザに別課題へ進む意思を確認する |

## 最初にやること — 前提チェック

作業開始前に 1 度だけ確認する。ユーザが「ログインしてる？」と明示的に聞いた場合もここに戻る。

1. `imoocs --version` が通ることを確認 (PATH に入っていない場合はインストールを案内)。
2. `imoocs auth status` を叩く。exit 0 なら MOOCs 認証済、exit 2 なら未ログインなので `imoocs setup` (初回) か `imoocs auth login` (再ログイン) を案内する。0/2 以外は config parse や network などの実エラーなので、そのまま障害として扱う。
3. 詳細が要るときだけ `imoocs doctor --format json` を叩き、認証状態や設定を読む。毎回叩かなくてよい。failure envelope が返ったら「未ログイン」ではなく診断自体の失敗として扱う。
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

MOOCs の API はレッスンごとの開講日 / 講義スケジュールを返さない。agent が「今日」「今週」「最新回」を推定しない。必ずユーザに具体的な lessonId (または URL) を聞いてから進む。連番の最新を決め打ちして資料を取りに行くと、未公開の回を掘ったり別レッスンの資料を返したりする。`imoocs course show <courseId>` で候補リストをユーザに提示して選ばせるのは OK。

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

`reference/submit-workflow.md` にチェックリストがあるので、提出系は必ず目を通す。骨子だけここに置く (`confirm` モード前提):

1. **課題を取得**: URL があれば `imoocs open <url>`、ID で分かっているなら `imoocs assignment show <courseId> <problemId>`。返ってきた `fields[]` を読んで、何を埋めれば良いか (textarea / text / radio / checkbox / file) を把握する。
2. **既存の currentValue を尊重**: 既に下書きがあるなら上書きする理由を明示。消さないほうが無難。
3. **ipynb を提出するなら agent 側の準備が要る**:
   - 全セルに実行結果があるか確認。無ければ再実行。
   - フォームが html を要求していたら `jupyter nbconvert --to html <path>.ipynb` を agent が実行し、成果物を upload に渡す。
   - 課題文 (markdown) と ipynb の実装 / 出力が整合しているか確認。ズレていたら修正 → 再実行 → 再確認。
4. **提出データをローカルで組み立てる**: テキスト / radio / checkbox 系は `{pid: value}` の JSON を一旦ローカルファイル (例: `/tmp/draft.json`) に書く。レビュー用の中間物はローカルに置いてユーザに内容を見せる。
5. **ユーザ確認を取ってから submit (stage)**: 明示的に「提出して」と言われるまで `submit` / `upload` は叩かない。内容に同意を得たら叩くが、`confirm` モードでは **この時点でサーバには送られない**（`$XDG_STATE_HOME/imoocs/drafts/` に stage されるだけ）:
   ```sh
   imoocs assignment submit <courseId> <problemId> --data @/tmp/draft.json
   imoocs assignment upload <courseId> <problemId> --pid <pid> <path>   # ファイル添付も同様に stage
   ```
   envelope `StagedResult` の `draftPath` / `answers` / `files` をユーザに見せて内容確認を取る (`imoocs assignment drafts show` でも同じ情報が読める)。
6. **ユーザに `push` の実行を依頼する**: `confirm` モードでは agent から `push` を叩くことはできない (非 TTY だと exit 3)。envelope の `hint` をそのまま伝え、ユーザに TTY で以下を叩いてもらう:
   ```sh
   imoocs assignment push <courseId> <problemId>
   ```
   対話プロンプトで `y` を押すと `put_answers` + 各 `post_file` が走り、成功で draft が消える。途中失敗時は draft が保持され、再 `push` で resume できる (answers/files は force=true で冪等上書き)。
7. **モード別の分岐**:
   - `auto` モード: `submit` / `upload` がそのまま即サーバ確定 (従来互換)。`push` は使わない (stage 自体が無いので `NOT_FOUND`)。
   - `confirm` モード未設定: `submit` / `push` とも exit 3 (`VALIDATION_ERROR`)。`imoocs setup` を案内。
8. **成否確認**: `push` が exit 0 を返したら `imoocs assignment show` を再度叩き、`status` と `fields[*].currentValue` / `uploadedFile` が埋まっていることを確認してからユーザに報告する。`push` 失敗時はサーバ側で answers だけ確定している可能性がある (complete な transaction ではない) — ユーザに「再 `push` で冪等に整合する」と案内。
9. **棚卸し**: 今回の課題以外にも同じコースに pending があれば `imoocs assignment list <courseId> --status pending` で一覧にして報告する。
10. **書き込みの実施状況を明示する**: 最終応答では「stage だけしたのか / ユーザの push で確定したのか / まだ何も叩いていないのか」を必ず 1 行で伝える。envelope の `staged` / `submitted` / `pushed` を根拠に書く。

### C. レッスン閲覧 / スライド PDF

1. `imoocs lesson show --url <url>` が既定で **markdown 本文 + embeds + 全課題 (AssignmentDetail) + Slides PDF (best-effort)** をまとめて返す。URL を貼られたらそのまま `--url` に渡す (skill 原則の「URL を手パースしない」に整合)。URL が手元に無く courseId / lessonId しか分かっていないなら positional 形 `imoocs lesson show <courseId> <lessonId> [--page <pageId>]` (`--page` 省略時は first page)。
2. **軽量化したいとき**: スライド PDF が要らなければ `--no-fetch-slides`、課題展開が要らなければ `--no-assignments`。どちらも付けなければ CLI が best-effort で全部取ってくる (Slide が取れなくても exit 0 維持、`embeds[*].fetchStatus = "skipped" | "failed"` が入るだけ)。
3. **スライドだけ単発で欲しいとき**: `imoocs slide fetch <embedUrl>`。保存先は config / `--out-dir` で `/tmp/imoocs/slides/` (default) / `cache` / 絶対パスから選べる。
4. PDF パスは `embeds[*].localPdfPath` に載る。必要なら Read tool で開いて読める (Linux なら `poppler-utils` が要る; 大きい場合は `pages` 指定で分割読み)。
5. 最上位原則 (§原則 2) で `imoocs open <url>` を先に叩いていた場合、URL が lesson / page なら `data.lesson` + `data.assignments` に同じ payload が既に入っているので、**lesson show を追加で叩き直さない**。
6. **授業の配布物 (zip / PDF / ノートテンプレ) は Drive フォルダから探す**。INIAD は本編コード・データ・配布資料をコース専用の Google Drive フォルダにまとめる運用で、スライド PDF 側は受講準備 (環境構築など) だけのことがある。取り方:
   1. `~/.config/imoocs/course-drive-folders.toml` を Read で開き、対象 `courseId` に紐づく `folderId` を引く。TOML が無い / 対象コースが未登録なら、先に `imoocs-drive-setup` skill を走らせてマッピングを作るようユーザに案内する (このスキルから呼ぶのではなく、ユーザの明示で起動する)。
   2. `imoocs drive list <folderId>` で中身を列挙。年度フォルダや講義回別サブフォルダがあれば `drive list <subFolderId>` で下りる。
   3. ファイル名 / lessonId / 更新日時から、該当レッスンの配布物として妥当な候補を 1–3 件に絞る (例: `ai-s02.zip`, `ai-s02-handout.pdf`)。確定できないときは候補をユーザに提示して選ばせる。勝手に確定しない。
   4. `imoocs drive fetch <fileId>` で取得。保存先は `$XDG_CACHE_HOME/imoocs/drive/<fileId>.<ext>` (永続)。zip なら agent 側で展開して中身を要約する。
   5. 補助: レッスン側の `embeds[*]` に `type: "google-drive"` が直接載っていれば、そちらが公式の配布物。TOML 経由より優先する。
7. Drive ネイティブ形式 (Docs/Sheets/Slides) は現状 API 経由で落とせないので、UI リンクをユーザに案内する。

## 落とし穴 (必ず守ること)

- **`NETWORK_RESTRICTED` (exit 7) は出席確認など一部のみ**。学外でこのエラーが出たら「学内 IP から再実行してください」とユーザに伝え、該当課題以外は普通に進める。全コースが学内限定ではない。
- **`submit` / `upload` / `push` の成否は exit code + envelope の `staged` / `submitted` / `pushed` で判定**。`confirm` モードの submit/upload は exit 0 + `staged:true, submitted:false` (サーバ未送信)。`auto` モードの submit/upload は exit 0 + `submitted:true` (サーバ確定済)。`push` 成功は exit 0 + `pushed:true, submitted:true`。`push` の exit 3 は非 TTY / プロンプトで `n` / EOF / config 未設定のいずれか (draft は保持)。勝手にリトライしたり「提出しました」と要約したりしない。
- **stage 中は local file を動かさない**。`upload` で stage したファイル絶対パスは `push` 時に `post_file` で読むので、stage 後に move / delete されると `push` が途中失敗する (draft は残るが file を戻すか別 upload で置き直す必要がある)。
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
