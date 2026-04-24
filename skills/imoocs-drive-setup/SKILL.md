---
name: imoocs-drive-setup
description: 履修中コースと Google Drive 授業資料フォルダの対応を $XDG_CONFIG_HOME/imoocs/course-drive-folders.toml に一括登録する imoocs の初期セットアップ補助。ユーザが `/imoocs-drive-setup` を明示的に呼んだ時だけ起動する。
---

# imoocs-drive-setup — 履修コース ↔ Google Drive 紐付けスキル

INIAD MOOCs で履修中のコースと、授業資料が置かれている Google Drive フォルダの対応を一度に解決し、`$XDG_CONFIG_HOME/imoocs/course-drive-folders.toml` に記録するための一括セットアップ手順書。以降の agent 作業が「○○のスライドの Drive 版を持ってきて」と言われた際にこの TOML を読むだけで該当フォルダへ即到達できるようにする。

## このスキルを使う場面

- `imoocs setup` を終えた直後の「仕上げ」ステップ
- 新年度開始時 / 履修登録が確定した直後
- 「Drive フォルダを登録して」「コースごとの Drive リンクを整理したい」「履修と Drive の紐付けを更新」と依頼されたとき
- 「最初のセットアップで Drive もやっておきたい」と言われたとき

### 使わない場面

- 特定コースのスライド PDF が欲しい / 課題を出したい / レッスン本文を読みたい
  → 既存 [`imoocs`](../imoocs/SKILL.md) skill の範疇。このスキルは**マッピング生成と更新**だけに責任を絞る。
- マッピングが既に出来ており Drive フォルダを開きたいだけ
  → `$XDG_CONFIG_HOME/imoocs/course-drive-folders.toml` を Read して URL を取り出すだけでよい。改めてこの skill を起動しない。

## 原則

1. **履修中コースは `imoocs course list` の結果そのまま**。MOOCs の `/courses/<year>` ページは認証済ユーザの履修コースのみをレンダリングするため、CLI 側でフィルタは不要。返ってきた `Course[]` を全件対象にする。
2. **Drive ルートは folder 名 `[受講生]講義資料` で発見する**。直下に `2024/`, `2025/`, `2026/` 等のサブフォルダがあり、その下に授業フォルダが並ぶ構造を前提とする。既存 `course-drive-folders.toml` に `driveRootFolderId` があればまずそれを再利用し、失敗時だけ名前検索で再発見する。
3. **Drive 操作は `imoocs drive` サブコマンドに集約**。`imoocs drive list <folderId>` / `imoocs drive fetch <fileId>` を使い、Drive API を直接叩いたり URL を手でパースしたりしない。`gws-drive` には委譲しない — MOOCs 側のログイン cookie で解決できる範囲はすべて `imoocs` 内で完結する。
4. **schema は N:M を許容**。1 コースに複数 Drive フォルダ (例: COT101「概論Ⅰ + 基礎演習Ⅰ」) は `[[courses.driveFolders]]` を複数並べる。複数コースが同じフォルダを共有 (例: HII201/UX104/UX108 の「デザイン理論」) は **同じ `id` を各 entry に書く** だけで表現する。1 コース 1 フォルダの 1:1 が満たせない場合に妥協で 1 つに絞る運用は禁止。
5. **マッチ確定はユーザに最終判断を委ねる**。完全一致 (`exact`) でも 1 件しか候補が無い場合でも、`unresolved` 以外の `matchStrategy` を付けるのは Step 5 の確認を経た後のみ。「partial 1 件だから自動採用」はしない。勝手に確定しない。
6. **冪等に振る舞う**。既存 TOML を読み込み、既に解決済みの行 (`matchStrategy ∈ {exact, user-confirmed}` かつ `driveFolders` が非空) は原則保持。ユーザが手で URL を書き換えている場合もそれを尊重する。
7. **中間スクリプトを書かない**。前回の trace で `/tmp/imoocs-setup/match.py` のような ad-hoc Python に逃げて、毎回違う正規化を発明する事故が起きた。マッチングは LLM (= あなた) が `imoocs course list` と `imoocs drive list` の出力を直接読み、頭の中で本書の正規化規則を適用して Markdown 表として組み立てる。jq / awk のワンライナーは可だが、別ファイル化したスクリプトは作らない。

## 前提チェック

作業開始前に 1 度だけ:

1. `imoocs --version` が通る (未インストールならインストール案内)。
2. `imoocs auth status` → exit 0 (MOOCs 認証済)。exit 2 なら `imoocs auth login` を先に案内して中断。0 / 2 以外なら config parse や network の実障害なので、ログイン不足ではなくエラーとして止める。
3. Google Drive に触れること。`imoocs auth login-google` が済んでいる状態で `imoocs drive search --exact "[受講生]講義資料"` が exit 0 / `success: true` で返れば OK。exit 2 なら `imoocs auth login-google` を案内。`gws-drive` 側の OAuth は **このスキルでは不要**。
4. 保存先ディレクトリを決定する。`$XDG_CONFIG_HOME` があればそこ、無ければ `$HOME/.config`。最終的な書き込み先は `$XDG_CONFIG_HOME/imoocs/course-drive-folders.toml`。

前提が揃っていないなら、この時点で中断してユーザに案内する。先に進まない。

## 実行手順

### Step 1. 履修コース一覧を取得

```sh
imoocs course list
```

- JSON envelope 固定。`data` は `Course[]` (`{ year, courseId, name, url }`)。
- exit 2 なら `imoocs auth login` を案内して中断。
- `--year` / `IMOOCS_YEAR` を指定する場合はユーザ確認を取ってから。既定 (現在年度) を優先。

### Step 2. 既存マッピングを読み込む

```sh
test -f "${XDG_CONFIG_HOME:-$HOME/.config}/imoocs/course-drive-folders.toml" \
  && cat "${XDG_CONFIG_HOME:-$HOME/.config}/imoocs/course-drive-folders.toml" \
  || echo "(none)"
```

- 既存エントリを記憶し、後の書き込みでマージ対象にする。
- 無ければ空状態から開始。

### Step 3. Drive root を発見して年度サブフォルダを特定

```sh
imoocs drive search --exact "[受講生]講義資料"
```

手順:

1. 既存 `course-drive-folders.toml` に `driveRootFolderId` がある場合は、まず `imoocs drive list <driveRootFolderId>` を試す。成功したらそのまま root として採用する。
2. 無い / 失敗した場合は `imoocs drive search --exact "[受講生]講義資料"` を実行する。
3. exact が 0 件なら `imoocs drive search "[受講生]講義資料"` で partial fallback。
4. 候補が複数ある場合は各候補に対して `imoocs drive list <rootId>` を叩き、`name` が `20xx` に一致する folder の件数を数える。単独最多の候補があればそれを採用する。
5. それでも複数ならユーザに候補提示して選んでもらう。

root を決めたら、その `data.items[]` から次を満たすものを `yearFolderId` として採用する:

- `kind == "folder"` (すなわち `mime == "application/vnd.google-apps.folder"`)
- `name` が対象年度 (`"2026"` 等) と一致。Drive 側は素の数字 4 桁の前提だが、`"2026年度"` のように suffix 付きで作られている可能性もあるので、**素の `"2026"` で一致しなければ `"2026"` を含む folder を partial で拾い、複数あればユーザに確認**する。

分岐:

- 1 件ヒット → 採用して `yearFolderId` を保持。
- 0 件 → 「この年度用のフォルダが Drive に見当たりません。`[受講生]講義資料` の構造が変わったか、対象年度がまだ作られていない可能性があります」と案内。
- 複数ヒット → 候補を提示して選んでもらう。

v1.1 以降 `imoocs drive list` は `nextPageToken` で全件取得する (XHR pagination) ため、旧来の 50 件 truncate を考慮する必要はない。`truncated` フィールドは後方互換で残るが常に `false`。

### Step 4. 年度サブフォルダ配下をコースと突き合わせる

#### 4.1 入力を集める

1. `imoocs drive list <yearFolderId>` を叩き、返った `data.items[]` から `kind == "folder"` のものだけ残す (ファイルは無視)。v1.1 以降 XHR pagination で全件取得されるため 50 件打ち切りは発生しない。
2. これで「コース名一覧 (Step 1 の `Course.name`)」と「Drive フォルダ名一覧」が手元に揃う。

#### 4.2 名前正規化規則 (適用順)

両側の文字列に同じ手順で適用する。**外部スクリプトに逃げず、頭の中で適用してから比較する**。

ステップ 1〜5 は両系列共通の前処理、ステップ 6a/6b で **比較用** と **トークン化用** の 2 系列のキーを作る。Step 4.3 の優先度 1〜2 (完全一致 / substring) は **比較キー (6a)** で、優先度 3 (トークン共通率) は **トークン列 (6b)** で判定する。

1. **NFKC 正規化** — 全角ローマ字 / 全角英数字を半角に揃える (`Ⅰ → I`, `＆ → &`, `／ → /`, `　 → ` (半角空白))。
2. **ローマ数字 ↔ アラビア数字の双方向写像**:
   - `I/V/X` 系 (NFKC 後の半角): `I=1, II=2, III=3, IV=4, V=5, VI=6, VII=7, VIII=8, IX=9, X=10`
   - 比較時は両側を **アラビア表記に揃える** (`演習III` → `演習3`, `デザイン理論 II` → `デザイン理論 2`)。
   - **境界判定**: 「ローマ数字とみなす I/V/X の前後が英字でない」場合のみ変換する。具体例:
     - `演習 III` → `演習 3` (前は空白、後は終端 → 変換)
     - `UX` の `X` → 変換しない (前が `U` という英字)
     - `UXデザインII` → `UXデザイン2` (`II` の前は和文字、後は終端 → 変換)
     - `UI` の `I` → 変換しない (前が `U` という英字)
     - `MAT202` の `I` → そもそも courseId 文字列に含まれない (`MAT` の `I` は無い、`MAT202` の数字は variant 判定の対象外)
3. **記号の正規化**: `&`, `/` はそのまま残し、**中黒 `・` は削除** (`コンピュータ・サイエンス` → `コンピュータサイエンス`)。`：`/`:` (全角/半角コロン)、`(`/`（` も半角に揃える (NFKC で済む場合が多い)。`_` (アンダースコア) は **トークン区切りとして扱う** (`デザイン理論_Design Theory` → 中の `_` は空白に置換、`Design Theory` 部分は和英併記の英訳として残す)。
4. **括弧内補助情報の削除**: `(...)` `（...）` で囲まれた年度範囲表記 (`(~1F1021)`, `(1F1022~)`, `(再履修クラス)`) は比較時に削除する。
5. **`/` 区切りの旧/新名併記の分割**: `MAT202「情報数学 / 情報連携のための数学Ⅲ」` のような形式は **`/` で 2 つの候補名に分割** し、両方を Drive フォルダ名と突き合わせる。どちらかでマッチすれば候補とする。
6a. **比較キー** (優先度 1-2 用): 連続空白を 1 つに圧縮 → 両端 trim → **半角空白を全部除去** → lowercase (`演習 I` と `演習I` を同一視: 共に `演習1`)。
6b. **トークン列** (優先度 3 用): 連続空白を 1 つに圧縮 → 両端 trim → 次の区切り文字集合で split → 空要素を除去 → 各トークンを lowercase。
    - 区切り文字: `半角空白`, `&`, `+`, `/`, `_`, `:` (NFKC で `＆ → &`, `＋ → +`, `／ → /`, `： → :` に揃う前提)
    - 例: `デザイン理論:UX基礎` → `["デザイン理論", "ux基礎"]`、`デザイン理論_Design Theory` → `["デザイン理論", "design", "theory"]`、`コンピュータサイエンス概論1 & 演習1` → `["コンピュータサイエンス概論1", "演習1"]`、`データマイニング論+データサイエンス演習2` → `["データマイニング論", "データサイエンス演習2"]`

#### 4.3 マッチング判定 (優先度順)

| 優先度 | 使うキー | 条件 | 候補に付ける `matchStrategy` 候補 |
|---|---|---|---|
| 1 | 比較キー (6a) | `course.name` のキーが Drive フォルダ名のキーと完全一致 | `exact` |
| 2 | 比較キー (6a) | 一方が他方の真部分文字列 (どちらか一方の含む / 含まれる) | `partial` |
| 3 | トークン列 (6b) | `course.name` (or 旧/新名候補) のトークン集合と Drive フォルダ名のトークン集合の **共通トークン数 / 短い側のトークン数 ≥ 0.5** | `partial` |
| 4 | — | 上記いずれもヒットしない | `unresolved` |

優先度 3 の「共通トークン」の定義: 2 つのトークンが **完全一致** または **一方が他方の真部分文字列** であるとき共通 1 と数える。例:
- `演習1` と `基礎演習1` → 共通 1 (前者が後者に含まれる)。これで COT101「概論1 & 演習1」 ↔ Drive「基礎演習1」が拾える。
- `デザイン理論` と `デザイン理論` → 共通 1 (完全一致)。HII201「デザイン理論:UX基礎」 ↔ Drive「デザイン理論_Design Theory」で `デザイン理論` トークンが両側にあるので 1/2 = 0.5 で hit。
- `ux基礎` と `ux` → 共通 1 (後者が前者に含まれる)。

旧/新名併記 (Step 4.2 step 5 で 2 候補に分割した場合) は、各候補について優先度 1〜3 を順に試し、いずれか hit すれば候補に積む。

#### 4.4 出力する Markdown 表

LLM は次の表をシナリオ全件分まとめて提示する (Python スクリプトを書かず頭で組み立てる):

```
| courseId | course.name (正規化後) | candidate folder(s) | tentative strategy |
|----------|------------------------|---------------------|--------------------|
| INI301   | 機械学習と人工知能     | 機械学習と人工知能  | exact              |
| INI303   | データサイエンス演習3&4 | データサイエンス演習3 | partial (Ⅳ 部分なし) |
| COT101   | コンピュータサイエンス概論1&演習1 | 概論1, 基礎演習1 | partial (1:N) |
| HII201   | デザイン理論:UX基礎    | デザイン理論       | partial (N:1 候補) |
| CV101    | 地理情報システム       | (none)             | unresolved         |
| ...      | ...                    | ...                | ...                |
```

`exact` 行も含めて全件 Step 5 の確認に回す。**「partial 1 件だから自動採用」「exact だから無確認確定」はしない**。

#### 4.5 慣例辞書 (INIAD 命名パターン)

マッチ判定の根拠として参考にする。あくまで heuristic で、最終確定はユーザ判断:

- **「概論」「演習」は別フォルダで運用されることが多い** — `COT101「概論 I & 演習 I」` は Drive 側で `概論Ⅰ` + `基礎演習Ⅰ` の 2 フォルダになっている前提で 1:N を疑う。
- **「III＆IV」「I-II」「論＋演習」のような連番・複数科目束ね表記** — Drive 側では片方だけ存在することがある (例: 「演習Ⅲ&Ⅳ」 vs 「演習Ⅲ」のみ)、または複数フォルダに分かれる (例: INI202「データ・マイニング論＋データサイエンス演習II」 → Drive 側「データマイニング論」「データサイエンス演習Ⅱ」の 2 フォルダ)。区切り文字 `&` / `＆` / `+` / `＋` はすべて Step 4.2 step 6b でトークン分割される前提。partial / 1:N として候補に出して、ユーザに「片方だけ採用か両方か」を確認。
- **「デザイン理論」系の共有フォルダ** — `HII201`/`UX104`/`UX108` のような複数科目で「デザイン理論_Design Theory」を共有する慣例。N:1 を疑い、共有でよいかユーザに確認。
- **旧名/新名併記 (`/` 区切り)** — `MAT202「情報数学 / 情報連携のための数学Ⅲ」` のように 1 コース 2 名前のことがある。Drive 側は古い側 or 新しい側の片方しか無いことが多い (両方ある場合は別コースなので 1:1 で解決)。
- **再履修クラス (courseId が `-2` 等のサフィックス)** — `SEM101-2` などは本科生用フォルダしか無いケースがある。共有可なら本科生フォルダを付ける、不可なら `unresolved` + `unresolvedReason = "pending-folder"`。
- **未開講系列** — `CV101〜CV111` などの系列で年度フォルダに該当が一切無い場合は `not-offered` を疑う (今年度カリキュラムに無い)。「履修登録には残るが資料が出ない」科目。
- **`INI`/`COS`/`COT` 等のコース ID と Drive フォルダ名は基本的に独立** (Drive 側は和名のみ)。コース ID 文字列での突合は期待しない。

### Step 5. 曖昧ケースをユーザに確認

Step 4 の表全件をユーザに提示し、**ケース別に必要な確認** を取る。1 シナリオ 1 質問にまとめて回答負担を下げる。

#### 5.1 ケース A: 1:1 (exact / 単一 partial)

```
A. これらは候補が 1 つに決まりました。`exact`/`user-confirmed` で確定してよいですか?
   - INI301 機械学習と人工知能 → 機械学習と人工知能 (exact)
   - INI201 データサイエンス基礎 → データサイエンス基礎 (exact)
   ...
   [Y] 全部確定 [N] 個別に確認
```

#### 5.2 ケース B: 1:N (1 コース ↔ 複数候補)

```
B. 以下は 1 コースに複数 Drive フォルダが該当します。どう扱いますか?
   1. COT101 概論 I & 演習 I
      [a] 概論Ⅰ
      [b] 基礎演習Ⅰ
      [a+b] 両方束ねる (Recommended) / [a のみ] / [b のみ] / [skip]
```

採用したフォルダを `[[courses.driveFolders]]` 配列に並べ、`matchStrategy = "user-confirmed"`。

#### 5.3 ケース C: N:1 (複数コース ↔ 1 候補)

```
C. 以下のフォルダは複数コースで共有候補に挙がっています:
   - 「デザイン理論_Design Theory」 → HII201 / UX104 / UX108 が候補
   [全部紐付け] (Recommended) / [HII201 のみ] / [UX104 のみ] / [skip]
```

採用された各 entry の `[[courses.driveFolders]]` に同じ `id` を書く (重複 OK)。

#### 5.4 ケース D: 旧/新名併記

```
D. MAT202「情報数学 (~1F1021) / 情報連携のための数学Ⅲ (1F1022~)」
   Drive 側には「情報数学」のみがあります。
   [採用] (旧名でも資料は同じ前提) / [skip]
```

#### 5.5 ケース E: 候補ゼロ (unresolved の理由分類)

候補ゼロのコースについては、`unresolvedReason` を 4 つから選ぶ:

```
E. 以下のコースは Drive フォルダが見つかりませんでした。理由はどれですか?
   - CV101〜CV111 (6 件)
     [not-offered] 今年度未開講 (Recommended) / [deferred] 学期途中で追加見込み
     / [pending-folder] 教員側で未作成 / [needs-user-input] わからないので次回判断
```

選んだ値を `unresolvedReason = "..."` に書く。`matchStrategy` は `"unresolved"` のまま。

ユーザがまとめて「全部 not-offered で」のように一括回答した場合はそのまま反映する。

### Step 6. TOML を書き込む

`$XDG_CONFIG_HOME/imoocs/course-drive-folders.toml` を以下の形式で生成・更新する:

```toml
# Generated by imoocs-drive-setup skill. Safe to edit by hand.
# Re-running the skill preserves hand-edited rows unless the user agrees to overwrite.
driveRootFolderId = "FAKE_DRIVE_ROOT_ID_SAMPLE_0001"

# 1:1 (典型)
[[courses]]
year = 2026
courseId = "INI301"
name = "機械学習と人工知能"
matchedAt = "2026-04-25"
matchStrategy = "exact"
[[courses.driveFolders]]
id = "FAKE_DRIVE_FOLDER_ID_SAMPLE_0001"
url = "https://drive.google.com/drive/folders/FAKE_DRIVE_FOLDER_ID_SAMPLE_0001"

# 1:N (1 コースに複数フォルダ: COT101 概論Ⅰ + 基礎演習Ⅰ)
[[courses]]
year = 2026
courseId = "COT101"
name = "コンピュータ・サイエンス概論 I & 演習 I"
matchedAt = "2026-04-25"
matchStrategy = "user-confirmed"
[[courses.driveFolders]]
id = "FAKE_FOLDER_GAIRON_I"
url = "https://drive.google.com/drive/folders/FAKE_FOLDER_GAIRON_I"
[[courses.driveFolders]]
id = "FAKE_FOLDER_KISO_ENSHU_I"
url = "https://drive.google.com/drive/folders/FAKE_FOLDER_KISO_ENSHU_I"

# N:1 (HII201/UX104/UX108 が共有「デザイン理論」) — 同じ id を 3 entry に書く
[[courses]]
year = 2026
courseId = "HII201"
name = "デザイン理論：UX基礎"
matchedAt = "2026-04-25"
matchStrategy = "user-confirmed"
[[courses.driveFolders]]
id = "FAKE_FOLDER_DESIGN_THEORY"
url = "https://drive.google.com/drive/folders/FAKE_FOLDER_DESIGN_THEORY"

[[courses]]
year = 2026
courseId = "UX104"
name = "デザイン理論 III"
matchedAt = "2026-04-25"
matchStrategy = "user-confirmed"
[[courses.driveFolders]]
id = "FAKE_FOLDER_DESIGN_THEORY"
url = "https://drive.google.com/drive/folders/FAKE_FOLDER_DESIGN_THEORY"

# Unresolved with reason (今年度未開講)
[[courses]]
year = 2026
courseId = "CV101"
name = "地理情報システム"
matchStrategy = "unresolved"
unresolvedReason = "not-offered"
```

`matchedAt` の生成タイミング:

- **新規解決時**: 今日の日付 (`YYYY-MM-DD`) を入れる。
- **既存解決行を保持する場合**: 既存値を維持する (touch しない)。
- **`unresolved` 行**: 設定しない (空欄 = フィールド省略)。

書き込み時の注意:

- 既存 TOML に同じ `(year, courseId)` のエントリがあれば**ユーザ編集を尊重**する。新しいマッチ結果と差異があれば、上書きの可否をユーザに確認してから反映。
- `matchStrategy ∈ {exact, user-confirmed}` で `driveFolders` が非空の行は再解決しない。差し替えたいときだけ別途ユーザ指示を受けて更新。
- `matchStrategy = "unresolved"` の行は **`unresolvedReason` の値で再走時の挙動が変わる** (Step 7「再実行時の挙動」参照)。
- ファイル生成先のディレクトリが無ければ `mkdir -p` で先に作る。

### Step 7. サマリ報告

最後に表で結果を報告:

| year | courseId | name | drive folders | strategy | reason |
|------|----------|------|---------------|----------|--------|
| 2026 | INI301 | 機械学習と人工知能 | [link](https://drive.google.com/...) | exact | — |
| 2026 | COT101 | 概論 I & 演習 I | 概論Ⅰ + 基礎演習Ⅰ (2 件) | user-confirmed | — |
| 2026 | HII201 | デザイン理論：UX基礎 | デザイン理論 (共有) | user-confirmed | — |
| 2026 | CV101 | 地理情報システム | (なし) | unresolved | not-offered |

`unresolved` の内訳件数 (`deferred` / `not-offered` / `pending-folder` / `needs-user-input`) もまとめて添える。

未解決が残っていれば、reason 別に案内:

- `deferred` / `pending-folder`: 「該当フォルダが Drive に用意されたタイミングでこのスキルを再実行してください」
- `not-offered`: 「今年度未開講の判断です。来年度以降に再走するか、開講確認後に手動で TOML を編集してください」
- `needs-user-input`: 「次回 `/imoocs-drive-setup` を起動した際にもう一度確認します」

## 失敗モードと対処

| 症状 | 対処 |
|------|------|
| `imoocs course list` が exit 2 (`AUTH_EXPIRED`) | `imoocs auth login` を案内してから再実行 |
| `imoocs course list` が exit 6/7 | ネットワーク回復後に再試行 (学外でも通るはず。exit 7 が出るのは限定的) |
| `imoocs drive list` が exit 2 | Google SSO セッション切れ。`imoocs auth login-google` を案内して再実行 |
| `imoocs drive search` / `list` が `Parse("Drive XHR endpoint may have changed upstream")` | Google 側で v2beta XHR の shape / endpoint / API key / query semantics が変わった可能性。CLI のアップデートを待つか issue 報告 |
| `imoocs drive search --exact "[受講生]講義資料"` が 0 件 | root 名変更、アクセス権不足、Google SSO 不完全の順で疑う |
| 年度フォルダが Drive に無い | `[受講生]講義資料` 配下の構造変更、または対象年度のフォルダ未作成を疑う |
| **大量のコースで完全一致が出ない (表記ゆれ)** | Step 4.2 の正規化規則を 1 つずつ確認。特にローマ数字↔アラビア数字、中黒削除、`/` 旧新名分割の見落としが多い。**Python スクリプトを書かずに** 表上で 1 件ずつ判定し直す |
| **コースフォルダが 0 件ヒット (1 系列丸ごと)** | その系列が今年度未開講の可能性大。Step 5.5 で `unresolvedReason = "not-offered"` を検討 |
| **再履修クラス (courseId にハイフン) のフォルダが無い** | 本科生用フォルダを共有してよいか確認。不可なら `unresolvedReason = "pending-folder"` |

## 再実行時の挙動

このスキルは冪等。2 回目以降は:

1. 既存 TOML の `[[courses]]` を全て読み込む。
2. `driveRootFolderId` があればまず再利用し、失敗時だけ root を再探索する。
3. `imoocs course list` の最新結果と突き合わせ、**新規 / 消失 / 変更** を分類する。
4. 既存の `exact` / `user-confirmed` 行 (かつ `driveFolders` 非空) は触らない (ユーザが上書きを明示した場合を除く)。
5. `unresolved` 行は `unresolvedReason` で挙動を分ける:
   - `deferred` / `pending-folder` / `needs-user-input` → 今回の Drive 検索結果で再解決を試みる
   - `not-offered` → 再走時もスキップ (年度が変わった or ユーザが明示再評価を要求した場合のみ再判定)
6. 新規履修コースは Step 3〜5 で解決する。

## 関連

- 既存 [`imoocs`](../imoocs/SKILL.md) skill — 課題提出 / スライド取得 / レッスン閲覧などの実作業はそちら。Drive 操作は `imoocs drive list` / `imoocs drive fetch` を使う。本スキルは CLI に手を入れず、外側で orchestration するだけ。
