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

## `NETWORK_RESTRICTED` (exit 7) の扱い

`CLAUDE.md` にも書かれている通り、これは出席確認課題と一部のみ。大半の課題は学外からでも触れる。

ユーザに伝える文例:

> この課題 (`ai-s02-attendance` など) は学内 IP 限定のようです。学内ネットワークか INIAD VPN に接続して `imoocs ...` を再実行してください。他の課題には影響しないので、そちらから片付けることもできます。

誤って「全コースが学内限定」と伝えない。

## ログイン切れの復帰

```sh
imoocs auth status
# exit 0 → MOOCs OK
# exit 2 → 未ログイン / セッション切れ
```

- `exit 2` なら `imoocs auth login`。初回やユーザ名を変える場合は `--username <s...>` / `--password-stdin`。
- Google (スライド PDF / Drive) 側の切れは `imoocs auth login-google`。`imoocs doctor --format json` の `googleAuthenticated` が false ならここを案内。
- 両方まとめて確認するなら `imoocs doctor`。テキスト要約でも十分な情報が出る。機械処理したいときは `--format json`。

## `submit` したのに `submitted: false` が返る

下書きには積まれているが確定されていない状態。envelope と stderr の notice をそのままユーザに引用して、どうするか判断を仰ぐ。agent 側で勝手に再試行したり、「提出しました」と要約したりしない。

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

仕様。`--format json` を付けても無視される (DESIGN.md 第 1 章)。agent は exit code と stderr で判断する:

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

