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
```

- `exit 2` なら `imoocs auth login`。初回やユーザ名を変える場合は `--username <s...>` / `--password-stdin`。
- Google (スライド PDF / Drive) 側の切れは `imoocs auth login-google`。`imoocs doctor --format json` の `googleAuthenticated` が false ならここを案内。
- 両方まとめて確認するなら `imoocs doctor`。テキスト要約でも十分な情報が出る。機械処理したいときは `--format json`。

## `submit` / `upload` が exit 3 (`VALIDATION_ERROR`) で止まる

`assignment.confirm = "confirm"` 設定下で以下のどれかが起きた場合、CLI は**API を呼ばずに中断**する (サーバ状態は変わらない):

- 非 TTY (agent / パイプ / CI) から呼ばれた
- TTY プロンプトで `n` を押した / EOF で閉じた

`error.hint` を読んでユーザに状況を伝え、どう進めるか判断を仰ぐ。勝手に再試行したり `--data` を書き換えて再送したりしない。選択肢は 2 つ:

1. TTY (対話シェル) から `imoocs assignment submit ...` を叩き直す
2. `~/.config/imoocs/config.toml` の `[assignment] confirm` を `"auto"` に変更 (agent に委任する意思があるときのみ)

その他の exit 3 は引数不備 (`--data` の JSON 形式 / 初回セットアップ未了) が原因。`error.hint` と `imoocs setup` / `imoocs assignment show` で分岐。

## スライド PDF が消えた / 見つからない

- 既定では `/tmp/imoocs/slides/<sha1>.pdf` に保存される。OS 再起動で消えるのが正しい挙動。
- 永続化したいなら config:
  ```toml
  [slides]
  out_dir = "cache"   # $XDG_CACHE_HOME/imoocs/slides/ に保存
  ```
  あるいは絶対パスを指定。`imoocs slide fetch --out-dir <path>` で単発上書きも可。
- `--no-cache` を付けると再取得する。

## `imoocs auth *` が JSON を返さない

仕様。`--format json` を付けても無視される。agent は exit code と stderr で判断する:

- `auth status` → 0 / 2
- `auth login` → 0 / 2
- `auth logout` → 0
- `auth export` → 0 (username と keyring 有無を text で出す)

機械的に認証状態を取りたいなら `imoocs doctor --format json` を使う。

## Drive が 50 件で切れる

`drive list` の envelope で `truncated: true` が立っていれば、HTML 初期ロードの 50 件制限に当たっている。51 件目以降は現 CLI では取れない (v2 で対応予定)。ユーザに Drive UI で直接見るか、`folderId` を絞って下位フォルダを列挙するよう案内する。

## 年度が違う

- `imoocs` は既定で MOOCs の現在年度を自動解決する。過去年度に触るなら:
  ```sh
  imoocs --year 2025 course list
  IMOOCS_YEAR=2025 imoocs course show COS201
  ```
- URL に年度が含まれている場合 (`/courses/2025/COS201`) は `imoocs open <url>` がそのまま年度を拾う。

