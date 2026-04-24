# トラブルシュート

`imoocs` が期待通りに動かないときの判断ガイド。SKILL.md から参照される。

## exit code 早見表

| code | `error.code` | 典型的な原因 | agent の反応 |
|---|---|---|---|
| 0 | - | 成功 | 続行 |
| 1 | `API_ERROR` | MOOCs 側 4xx / 5xx、レスポンス形式の異常 | `hint` を読む。一時的障害の可能性があれば少し置いて再試行、再現するならユーザに報告 |
| 2 | `AUTH_EXPIRED` | Keycloak セッション / cookie の失効 | `imoocs auth login` を案内。必要なら `imoocs auth login-google` も |
| 3 | `VALIDATION_ERROR` | 引数欠落、設定不備、`--data` の JSON 形式不備 | `error.hint` を読み、初回セットアップが未了なら `imoocs setup`、JSON 不備なら `--data` の内容を見直す |
| 4 | `NOT_FOUND` | URL / courseId / problemId / lessonId の誤り | 上位コマンド (`course list`, `course show`, `lesson show`) で辿り直して正しい ID を再発見 |
| 5 | `INTERNAL_ERROR` | CLI 側のバグや未対応 edge case | スタックトレースを付けて issue 報告を勧める。ユーザには無理に再試行させない |
| 6 | `NETWORK_ERROR` | DNS / TCP / TLS 障害、proxy | ネットワーク復帰後に再試行。断続的なら一呼吸置く |
| 7 | `NETWORK_RESTRICTED` | `/status: "network"` 応答。**学内 IP 限定リソース** | 「学内 / VPN で再実行を」と案内。他の課題は普通に進められる |
| 8 | `NON_PUBLIC` | `assignment show` で未公開課題に触れたとき (`/status` / `/problem` / `/answers` が 403) | 「解禁を待つ」と案内するか、ユーザに別課題へ進む意思を確認する |

## `NETWORK_RESTRICTED` (exit 7) の扱い

`CLAUDE.md` にも書かれている通り、これは出席確認課題と一部のみ。大半の課題は学外からでも触れる。

ユーザに伝える文例:

> この課題 (`ai-s02-attendance` など) は学内 IP 限定のようです。学内ネットワークか INIAD VPN に接続して `imoocs ...` を再実行してください。他の課題には影響しないので、そちらから片付けることもできます。

誤って「全コースが学内限定」と伝えない。

## `NON_PUBLIC` (exit 8) の扱い

`assignment show` が exit 8 (`error.code: "NON_PUBLIC"`) を返すのは、対象課題が **まだ解禁されていない / 公開されていない** ときの正常な応答。MOOCs サーバ側で `/status` / `/problem` / `/answers` のどれかが 403 を返したケースで、通信障害ではない。

典型例:
- `atnd-lecture-01` など出席確認が、その週の講義が行われる前 (まだ問題 HTML が生成されていない)
- `ai-03-quiz` など、講義スケジュールに沿って解禁を待っている課題
- `assignment list --status nonpublic` で `derivedStatus: "nonpublic"` として見えていた課題を、show で掘ろうとしたとき

ユーザに伝える文例:

> この課題はまだ公開されていないようです (exit 8 / NON_PUBLIC)。講義開始後か、教員が解禁したタイミングで再度取得できるはずです。いま手を付けられる他の課題を優先しますか?

`exit 4 / NOT_FOUND` (URL・ID の誤り) とは意味が違う。URL 誤りと混同して「ID を再確認して」と返さない。

## ログイン切れの復帰

```sh
imoocs auth status
# exit 0 → MOOCs OK
# exit 2 → 未ログイン / セッション切れ
# other  → config parse / network / internal error
```

- `exit 2` なら `imoocs auth login`。初回やユーザ名を変える場合は `--username <s...>` / `--password-stdin`。
- 0 / 2 以外は「未ログイン」ではなく実障害。stderr のエラーをそのまま読み、config の破損やネットワーク障害として扱う。
- Google (スライド PDF / Drive) 側の切れは `imoocs auth login-google`。`imoocs doctor --format json` の `googleAuthenticated` が false ならここを案内。
- 両方まとめて確認するなら `imoocs doctor`。テキスト要約でも十分な情報が出る。機械処理したいときは `--format json`。ただし config/TOML/network 障害では success envelope ではなく failure envelope を返す。
- セッションだけ切り直したいなら `imoocs auth logout` (keyring + `cookies.json` のみ破棄、`config.toml` は残るので username は再入力不要)。

## 完全初期化 (`imoocs reset`)

「別アカウントで検証したい」「設定ごとおかしいのでまっさらにしたい」「PC 譲渡前に痕跡を消したい」といった要望に使う。`auth logout` より広く、設定 / cache / draft まで消せる。

先に `--dry-run` で何が消えるか確認してから実行するのが安全:

```sh
imoocs reset --dry-run
imoocs reset --scope all --yes          # 全消し (CI / agent はこれ)
imoocs reset --scope auth --yes         # auth logout と等価
imoocs reset --scope config --yes       # username / preference だけリセット
imoocs reset --scope cache --yes        # cookies / drive cache / slides を掃除
imoocs reset --scope drafts --yes       # 未 push の提出物を破棄
imoocs reset --scope auth,cache --yes   # CSV 複数指定 OK
```

確認プロンプトは default **No**。非 TTY で `--yes` を付けずに呼ぶと exit 3 で止まる (agent 事故防止)。keyring backend の障害が出た場合は **config.toml を残したまま** 他のスコープを消し、exit 5 を返す (username を維持して次回リトライで keyring entry を再度狙えるように)。

**safety note**:

- `--scope` は値が必須。`imoocs reset --scope --yes` のようなタイポは exit 2 で clap が reject する (scope 省略なら `all` と同等)。
- `reset --scope cache` の `slides_dir` 削除は **`<cache_dir>/slides` または `/tmp/imoocs/slides`** 配下に限る。ユーザが `slides.out_dir` に共有フォルダを書いていても巻き込まない (skip + 通知)。手で消す前提。
- 壊れた `config.toml` (parse error) があっても `reset --scope config` は通る。config の復旧経路として使える。

## `assignment push` が exit 3 (`VALIDATION_ERROR`) で止まる

`push` は stage した draft をサーバに確定送信するコマンド。以下のどれかで中断する (`put_answers` / `post_file` は呼ばれていない、draft は保持):

- 非 TTY (agent / パイプ / CI) から呼ばれた
- TTY プロンプトで `n` を押した / EOF で閉じた
- `assignment.confirm` が未設定 (`imoocs setup` を走らせる)

agent の正しい反応: `error.hint` / `error.message` をそのままユーザに伝え、TTY から `imoocs assignment push <courseId> <problemId>` を叩いてもらうよう依頼する。draft の中身を先に見せたいなら `imoocs assignment drafts show <courseId> <problemId>` で表示できる。勝手に再試行しない。

注記: `confirm` モードの `submit` / `upload` は **exit 3 では止まらない**。サーバに送らずローカル draft に stage するだけなので、exit 0 + envelope `staged: true, submitted: false` が正常応答。「submit は成功したがサーバに反映されない」のは仕様通りで、`push` を叩くまで finalise されない。

その他の exit 3 は引数不備 (`--data` の JSON 形式 / 初回セットアップ未了) が原因。`error.hint` と `imoocs setup` / `imoocs assignment show` で分岐。

## `assignment push` が exit 1 / 6 で途中失敗する

`push` は `put_answers` → 各 `post_file` を順次叩く複数 HTTP リクエストで、transaction は無い。途中で 5xx / ネットワーク断が起きるとそこで止まり、**サーバ側は部分確定の可能性あり** (answers は送れて files の一部が未送、など)。

agent の対応:

- draft は自動的に保持される (`error.message` に "Draft retained at \<path\>. Re-run `imoocs assignment push` to resume." が入る)
- ユーザには「サーバ側で answers だけ確定している可能性がある。再 `push` で冪等に整合する」と案内
- `put_answers` は `force=true` で answers を上書きする冪等操作、`post_file` も pid 単位で上書きなので、再 `push` で副作用なく resume できる
- 連続で失敗するなら MOOCs 側の一時障害を疑って時間を置く

## スライド PDF が消えた / 見つからない

- 既定では `/tmp/imoocs/slides/<sha1>.pdf` に保存される。OS 再起動で消えるのが正しい挙動。
- 永続化したいなら config:
  ```toml
  [slides]
  out_dir = "cache"   # $XDG_CACHE_HOME/imoocs/slides/ に保存
  ```
  あるいは絶対パスを指定。`imoocs slide fetch --out-dir <path>` で単発上書きも可。
- `--no-cache` を付けると再取得する。

## `embeds[*].fetchStatus` が `"skipped"` / `"failed"` で返る

`lesson show` / `open <lesson-url>` は既定で埋め込み Google Slides の PDF を
best-effort で取得する。失敗しても全体 exit は 0 を維持し、該当 embed の
`fetchStatus` に以下のいずれかが入る:

- `"ok"` — 取得成功。`localPdfPath` / `sizeBytes` / `pageCount` / `fetchedAt` が埋まる
- `"skipped"` — Google SSO が未ログインで取りに行かなかった。`imoocs auth login-google` を案内
- `"failed"` — ネットワーク / pubembed レイアウト変更 / PDF 合成エラー等の実障害。stderr の warn ログを読み、再試行か `imoocs slide fetch <embedUrl> --no-cache` で単体デバッグ

agent の反応:

- `skipped` → `imoocs auth login-google` をユーザに案内。PDF が要らない用途なら `--no-fetch-slides` を付けて再実行してよい
- `failed` → 同じ URL を `imoocs slide fetch` で単発叩いて error 詳細を拾う (こちらは exit 2/6 で落ちる)
- どちらも `markdown` / `assignments` は正常に返っているので、テキスト情報を先に使って作業継続できる

## `imoocs auth *` が JSON を返さない

仕様。`--format json` を付けても無視される。agent は exit code と stderr で判断する:

- `auth status` → 0 / 2（実障害時は他の exit code）
- `auth login` → 0 / 2
- `auth logout` → 0 (keyring + `cookies.json` のみ破棄、`config.toml` は残す)
- `auth export` → 0 (username と keyring 有無を text で出す)

機械的に認証状態を取りたいなら `imoocs doctor --format json` を使う。failure envelope も返りうるので、必ず top-level `success` を確認する。

## Drive が 50 件で切れる

`drive list` の envelope で `truncated: true` が立っていれば、HTML 初期ロードの 50 件制限に当たっている。51 件目以降は現 CLI では取れない (v2 で対応予定)。ユーザに Drive UI で直接見るか、`folderId` を絞って下位フォルダを列挙するよう案内する。

## 年度が違う

- `imoocs` は既定で MOOCs の現在年度を自動解決する。過去年度に触るなら:
  ```sh
  imoocs --year 2025 course list
  IMOOCS_YEAR=2025 imoocs course show COS201
  ```
- URL に年度が含まれている場合 (`/courses/2025/COS201`) は `imoocs open <url>` がそのまま年度を拾う。
