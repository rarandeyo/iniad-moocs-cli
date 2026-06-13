# agent-browser 移行 Phase 0 実機調査メモ

`/home/rarandeyo/.claude/plans/api-eager-salamander.md` の Phase 0 で潰す Q1〜Q17 の調査結果を記録する。Phase A 着手前にすべての Q が「確定」または「Phase 実装時に観察」のいずれかに振り分けられている状態を目指す。

調査環境:
- `agent-browser --version`: 0.21.2 (mise + npm 経由 install、`~/.local/share/mise/installs/node/24.13.1/bin/agent-browser`)
- 検証対象 MOOCs ページ: `https://moocs.iniad.org/courses/2026/INI301/AI-s01/09` (課題1 `ai-s01-assign1`)
- 検証対象 Slides: `https://docs.google.com/presentation/d/e/2PACX-1vSlYKxN1xyKkW23l6yqdhuZkh6HfPJsfIWle-ZX6UUU1hz-IHGBziNRKD_ffSOtkA/pubembed`

---

## 確定済 (Plan モード中の実機検証で済んだもの)

### Q1: Keycloak ログイン画面の DOM (確定)

- `https://moocs.iniad.org/auth/iniad` は **`https://accounts.iniad.org/auth/realms/master/protocol/openid-connect/auth?client_id=iniad-moocs&...`** にリダイレクトされる (Keycloak)
- ページタイトル: `INIAD ID Manager`
- a11y snapshot (interactive, scope=`form.form-signin`):
  ```
  - textbox "ユーザー名" [ref=e3]
  - textbox "パスワード" [ref=e4]
  - checkbox "Remember my username" [ref=e6]
  - button "LOG IN" [ref=e5]
  ```
- CSS セレクタは Q17 で実機検証する必要あり (`input[name=username]`, `input[name=password]`, `form.form-signin button[type=submit]` の予想)

### Q3: 課題ページの提出機構 (確定、最重要発見)

- `.problem-container` は `<form>` を**持たない**
- 以下の data 属性を持つ:
  - `data-problem="ai-s01-assign1"`
  - `data-lang="ja"`
  - `data-urlprefix="/assignments/2026/INI301/ai-s01-assign1"`
- JS が prefix から URL を組み立てて `/answers` (PUT) や `/file/<pid>` (POST) を **XHR で叩く**
- CSRF は jQuery が `meta[name=csrf-token]` を自動付与する慣行
- 「提出」ボタンを click すると内部 API が叩かれるが、これは人間がブラウザを使うときと同一動作 → 倫理要件に合致
- 課題コンテナの initial outerHTML: **3.1KB** (コンテキスト消費極小)
- 初期構造: `.problem-spinner` (139B) + `.problem-coverpage` (294B, `button.start-answer` を含む) + `.problem-contentpage` (2.5KB, 本文 AJAX 注入先)
- ボタン体系:
  - `button.start-answer "問題を開く"` (初期表示、click で AJAX 発火)
  - `button.file-trigger-btn "ファイルをアップロード"` (file input トリガ、複数可)
  - `button.submit-answer "提出"`
- pid 命名: `<textarea name="p01">`, `p02`, ... (=内部 API の pid)

### Q9: `batch --json` の per-command 結果 shape (確定)

```json
[
  {"command": ["open", "https://..."], "error": null, "result": {"title": "...", "url": "..."}, "success": true},
  {"command": ["wait", "--load", "domcontentloaded"], "error": null, "result": {"state": "domcontentloaded"}, "success": true},
  ...
]
```

`success: bool` で per-command 成否判定。`--bail` で 1 つ失敗時に後続停止 (デフォルトは継続)。

### Q10: `upload` への `--allow-file-access` 要否 (実機 help で確認、e2e で最終確認予定)

agent-browser 0.21.2 の `upload --help` には `--allow-file-access` の言及なし。`<input type=file>` への upload には不要と判明。`--allow-file-access` は `file://` URL アクセス専用 (公式 help より)。

実 INIAD 課題でのファイル upload 動作確認は **Phase C 着手時に destructive test として実施**。

### Q11: `download` コマンド (確定)

`download <selector> <path>` — **selector を click してダウンロードを発火する形式**。URL 直 dl は不可。

URL を navigate して自動 download を待ちたいときは `open <url>` + `wait --download <path>` の組み合わせを使う。

### eval の制約 (確定、auth/Slides 両方で重要)

- agent-browser 0.21.2 の `eval` は **top-level await が SyntaxError**
- 公式パターン: `Promise.then(() => { window.__flag = true; }); 1` で flag を立てて `wait --fn "window.__flag === true"` で完了待ち (実機検証済)
- 複数行スクリプトは `eval --stdin` で heredoc 受け取り可能 (`-b, --base64` でシェルエスケープ回避)

### pdf コマンドの制約 (確定)

- agent-browser 0.21.2 の `pdf <path>` には **paperWidth/Height/landscape/margin フラグが無い**
- CSS `<style>@page { size: 10in 5.625in; margin: 0 }</style>` を inject する手法は **実機検証で否定** (Chrome デフォルトの A4 縦が優先され、2 page になった)
- **解決策**: `agent-browser set viewport 1280 720` を pdf 前に実行 → 1 page 16:9 PDF が生成される (pdfinfo で確定)

### state コマンド (確定)

- `agent-browser state save/load/list/show/rename/clear/clean` 全部実在 (実機 help で確認)
- 自動: `--session-name <name>` で auto-save/restore
- 暗号化: `AGENT_BROWSER_ENCRYPTION_KEY` (64 hex chars, AES-256-GCM) または agent-browser 自動生成 (`~/.agent-browser/.encryption-key`)

### auth-vault 内部構造 (確定)

- 保存先: `~/.agent-browser/auth/<name>.json` (0o600)
- 構造: `{authTag, data, encrypted: true, iv, version: 1}` (AES-256-GCM envelope)
- 鍵: `~/.agent-browser/.encryption-key` (65 bytes = 64 hex chars + newline, 0o600) が初回保存時に**自動生成**
- 環境変数 `AGENT_BROWSER_ENCRYPTION_KEY` 未設定でも問題なく動作

---

## 未確定 (Phase 0 で潰す残課題)

### Q2: Google SAML chain がヘッドレスで全自動進行するか (確定 — 部分自動、speedbump で 1 click 必要)

**状態**: 確定。SAML chain は ACS まで自動進行、最後の本人確認 (speedbump) で 1 click 必要

**実機検証結果**:

1. **agent-browser ヘッドレスで** `https://accounts.google.com/samlredirect?domain=iniad.org` を open
2. SAML chain は途中まで自動進行 (saml-post-binding / hiddenpost form は JS auto-submit で通過)
3. **`https://accounts.google.com/speedbump/samlconfirmaccount`** で停止 (Google の本人確認ダイアログ)
4. speedbump ページの DOM:
   ```
   - heading "本人確認" [@e1]
   - button "続行" [@e2]
   - button "このアカウントに心当たりがない" [@e3]
   ```
   文言: 「表示されているアカウントがご自身のアカウントであることをご確認ください。」
5. **`click @e2` ("続行") で SAML 完了** → `https://myaccount.google.com/?utm_source=sign_in_no_continue` に到達

**Phase A2 実装パターン**:
```jsonc
[
  ["open", "https://accounts.google.com/samlredirect?domain=iniad.org"],
  ["wait", "--load", "networkidle", "--timeout", "30000"],
  ["get", "url"]
]
// もし URL が "speedbump/samlconfirmaccount" を含むなら:
[
  ["snapshot", "-i"],
  // "続行" ボタンの ref を解決して click
  ["find", "text", "続行", "click"],
  ["wait", "--load", "networkidle", "--timeout", "30000"]
]
// 最終確認:
[["open", "https://myaccount.google.com"], ["get", "url"]]
// → host が "myaccount.google.com" なら成功
```

**注意**:
- `wait --url "**myaccount.google.com**"` の glob パターンは効かなかった (URL pattern matching の glob spec は要追加調査)
- 代わりに `wait --load networkidle` + 別途 `get url` の host 確認パターンを推奨
- speedbump は初回ログイン時のみ表示される想定。2 回目以降は出ない可能性 (要追跡)

### Q4: 採点済課題の `assessment` ブロックの class/構造 (部分確定)

**状態**: セレクタと構造は確定。採点後の `##` 置換挙動だけ Phase B 実装時に観察

**検証結果**: `.problem-container .problem-contentpage .problem-assessment` で取得可能 (実機検証済)

DOM 構造:
```html
<div class="problem-assessment">         <!-- 未採点時は display: none -->
  <hr>
  <div class="panel panel-primary">      <!-- 採点済時に表示 -->
    <!-- 内容テキスト: "あなたの得点##/## コメント##" -->
    <!-- ##/## が点数、コメント## が採点者コメントに JS で置換 -->
  </div>
</div>
```

- `style.display` または `offsetParent === null` で「採点済 / 未採点」を判定可能 (visible:false なら未採点)
- `##` placeholder は採点後 JS が実値に置換するので、`.panel-primary` の textContent をパースして抽出
- AI-s01-assign1 は採点表示エリア自体は DOM に**常に存在**するが visible:false (未採点状態)

**残 TODO (Phase B 実装時)**:
- [ ] 実採点済課題で `.panel-primary` 内の点数表示パターンを観察 (例: `<span class="mark">5</span> / <span class="fullmark">10</span>`)
- [ ] コメント部分のセレクタ (`<div class="comment">` 等)
- [ ] fixture (採点済 HTML スナップショット) を `crates/imoocs-browser/tests/fixtures/assessment_graded.html` に保存

### Q5: NETWORK_RESTRICTED の DOM 表示文言 (closed 状態は確定、network 状態は要追加調査)

**状態**: Closed 状態の判定パターンは確定 (副産物)。NETWORK_RESTRICTED 専用文言は学外環境調査が必要

**Closed 状態の副産物発見**: AI-s01-assign1 (締切後の課題) の `.problem-contentpage` 内に:
```html
<div class="alert alert-warning problem-closed-only">
  現在回答を受け付けていません。
</div>
```
- セレクタ: `.problem-container .alert.problem-closed-only`
- 文言: `現在回答を受け付けていません。` (締切過ぎ)
- これで `AssignmentStatus::Closed` 判定が可能

**NETWORK_RESTRICTED 専用文言の調査**: 学外 IP で `atnd-*` 系を開いたときの専用文言 / class は未確認。`.problem-network-only` 等の慣例セレクタの可能性あり (`problem-closed-only` の命名規則から類推)

**残 TODO**:
- [ ] 学外環境で `atnd-*` 課題を開いて `.problem-network-only` / `.alert-danger` / 文言「学内ネットワーク」等を確認
- [ ] 観察できなければ Phase B 実装で「`.alert[class*="only"]` で取得 → 文言で分類」のヒューリスティクスを採用

### Q6: 提出後の成功 toast/文言 (Phase C 実装時に observation、予測あり)

**状態**: Phase C 着手時に destructive test で観察。MOOCs の Bootstrap 慣例から `alert-success` クラスが最有力候補

**Phase C 実装時の観察手順**:
1. テスト用の再提出可能な open 課題を特定 (例: 自由課題系)
2. `imoocs assignment submit` の destructive 経路を browser 経由で実行
3. 提出ボタン click 直後の DOM 変化を `eval` で記録:
   ```js
   // 仮: 提出後の toast/alert を取得
   document.querySelectorAll('.alert-success, .alert-info, .toast, [class*="success"]').forEach(e => console.log(e.outerHTML.slice(0,200)));
   ```
4. 観察した文言を `wait --text "..."` の引数に確定

**予測** (`.problem-closed-only` と同じ命名規則から類推):
- 成功時: `.alert.alert-success` で `提出しました` / `保存しました` 系
- 失敗時: `.alert.alert-danger` で `エラー` 系
- バリデーション: `.alert.alert-warning` で `回答が空です` 系

**Phase 0 では確定しない** (destructive)

### Q12: Slides の multi-slide PDF 化戦略 (戦略 A 採用で確定)

**状態**: 採用戦略確定 (戦略 A)。実装テストは Phase D0 spike で

**戦略 B (公式 export) の検証結果**: ❌ **使用不可**
- `https://docs.google.com/presentation/d/e/2PACX-1vSlYKxN1.../export/pdf` を Chrome MCP ログイン済みセッションで navigate
- レスポンス: `ページが見つかりません` (`title: "ページが見つかりません"`, content-type: text/html, 404 相当)
- 本文: 「リクエストされたファイルは存在しません」
- 結論: `/d/e/<encoded>/` 形式 (published) は export 機能が**対応外**

**戦略 A (per-slide pdf + lopdf マージ) の手がかり**: ✅ **採用可能**
- Chrome MCP で `pubembed?start=false&loop=false` を開くと、JS が自動で `?slide=id.p1` をクエリ文字列に追加した (タブタイトルから確認)
- つまり pubembed は `?slide=id.p<N>` (or `?slide=id.gXXX` の format) で個別ページ指定可能
- スライド総数の取得方法は要追加調査 (DOM か API か)

**結論**:
- **戦略 A 採用**: pubembed `?slide=id.p<N>` を 1 枚ずつ navigate → `set viewport 1280 720` + `pdf` → `lopdf` マージ
- **`lopdf` 依存は残置** (Phase E では削除しない)
- `pdf-writer` は現状未使用なので削除可
- `usvg`, `svg2pdf`, `unicode-escape` は削除可 (SVG 抽出経路を捨てるため)

**残 TODO (Phase D0 spike で実機検証)**:
- [ ] スライド総枚数を取得する方法 (`document.querySelectorAll('svg.punch-viewer-svgpage').length`? or punch global variable?)
- [ ] 各 `id.p<N>` の navigate 後にスライドが完全に切り替わるまでの安定待機方法
- [ ] 30 枚スライドで何秒/何 spawn かかるかの計測
- [ ] agent-browser ヘッドレスで Google 認証済み state を持って同等再現できるか (Phase A2 完了後に検証)

### Q13: Web フォントロード待ちの信頼性 (Phase D0/D1 spike 時に検証)

**状態**: Google ログイン済みのヘッドレス session で pubembed を開く必要があるため、Phase A2 完了後の D0 spike で検証

**検証手順 (Phase D0 で実施)**:
1. Phase A2 で `agent-browser auth save google` + `auth login google` まで完成させる
2. pubembed `/presentation/d/e/<id>/pubembed?slide=id.p1` を navigate
3. `set viewport 1280 720`
4. `eval "document.fonts.ready.then(() => Promise.all(Array.from(document.images).map(i => i.complete ? Promise.resolve() : new Promise(r => { i.onload=i.onerror=r; setTimeout(r,5000); }))).then(() => { window.__ready = true; })); 1"`
5. `wait --fn "window.__ready === true" --timeout 30000`
6. `pdf /tmp/slide-test.pdf`
7. `pdfinfo` + 目視で日本語フォントの有無確認、空白・置換文字検出

**Phase 0 では確定しない** (Google 認証必要)

### Q15: Google ヘッドレス検出による reCAPTCHA 強制の有無 (確定 — 検出されない)

**状態**: 確定。INIAD アカウントの SAML 経由ログインでは reCAPTCHA / `signin/challenge` への redirect は**発生しなかった**

**実機検証結果** (Q2/Q17 と同セッション):
- agent-browser ヘッドレス + INIAD SAML 経由で **reCAPTCHA や 2FA challenge は出ない**
- Google 側が見せたのは「初回本人確認 (speedbump)」のみで、これは headless 検出ではなく標準フロー
- 結論: **headed fallback はオプション扱いに格下げ可能**

**Phase A2 設計への影響**:
- 通常運用では headless で SAML 経由ログインが完結 (speedbump click 込み)
- headed fallback は将来の例外時 (Google policy 変更や 2FA 有効化時) のフェイルセーフとして残す
- Plan の §5.3 headed フォールバック設計は維持するが、**標準フローではない**ことを明示

**注意点**:
- 今回の検証は INIAD 経由 (SAML federated login)。Google 直接 login (`accounts.google.com/signin` で password 入力) はヘッドレス検出される可能性が高いので、SAML 経由のみを採用する設計が正解

### Q16: 「問題を開く」click 後の本文ロード完了判定 (確定、ただし予想と異なる)

**状態**: 確定。`.markdown-block` ではなく `.problem-contents` セレクタを使う

**検証結果**: AI-s01-assign1 を fresh ナビ → `button.start-answer` click → 10 秒待っても `.markdown-block` は **0 個のまま**。実際の DOM 構造を確認した結果、**`.markdown-block` クラスは MOOCs では使われていない** (Plan の予想は誤り)

**実態の DOM 構造** (`.problem-contentpage` 内):
- `.alert` (status 表示: closed/network 等の状態警告)
- `.problem-contents` (本文 + textarea。`<h3>` 見出し + `<p>` 説明 + `<textarea class="form-control" name="pNN">` の繰り返し)
- `<hr>`
- `<p>` (footer)
- `button.btn` (提出ボタン)
- `.problem-assessment` (採点エリア、未採点時 display:none)

**ロード完了判定の正しい実装**:
```js
// Phase B 実装で使う wait --fn 条件
wait --fn "(() => {
  const cp = document.querySelector('.problem-container .problem-contentpage');
  if (!cp || getComputedStyle(cp).display === 'none') return false;
  // 子要素のいずれかが描画されたか (本文 or 状態 alert or 採点)
  return cp.querySelector('.problem-contents, .alert, .problem-assessment') !== null;
})()"
```

**注意**: 「問題を開く」は**初期非展開状態**用 (`.problem-coverpage` が visible)。締切後・採点後など状態によっては coverpage が既に display:none で、contentpage が初期から見えていることがある (AI-s01-assign1 がそのケース)。したがって click は **`button.start-answer` が visible のときだけ**実行する。

**残 TODO**:
- [ ] 初期非展開状態の課題 (まだ「問題を開く」が表示されている課題) で click → AJAX 完了の所要時間を計測 (今回は既に展開済の課題でしか確認できなかった)
- [ ] 初期展開済課題でも `.problem-contents` 出現待ちで問題なく Phase B が動作するか統合テスト

### Q17: `agent-browser auth save moocs` の Keycloak セレクタ安定性 (確定)

**状態**: 確定。`#username` / `#password` / `#kc-login` で完全動作

**実機検証結果**:

1. **誤りの予想**: 当初 `input[name="username"]` / `input[name="password"]` / `form.form-signin button[type=submit]` を予想したが、Keycloak の submit は **`<input type="submit" id="kc-login">`** (= `<button>` 要素は存在しない)
2. **正しいセレクタ**:
   ```
   --username-selector '#username'
   --password-selector '#password'
   --submit-selector '#kc-login'
   ```
3. 実機動作確認:
   - `auth save moocs-test` 成功
   - `auth login moocs-test` 成功 → `loggedIn: true`
   - 直後の URL が `https://moocs.iniad.org/courses` (Keycloak → MOOCs リダイレクト)
   - `/account` への navigate → final URL も `/account` ← MOOCs ログイン状態確定

**Keycloak ページの実態** (`accounts.iniad.org/auth/realms/master/protocol/openid-connect/auth`):
```html
<form id="kc-form-login" class="form-signin" action="...">
  <input name="username" id="username" type="text" class="form-control input-lg" placeholder="ユーザー名">
  <input name="password" id="password" type="password" class="form-control input-lg" placeholder="パスワード">
  <input name="rememberMe" id="rememberMe" type="hidden">
  <input id="rememberMyUsername" type="checkbox">
  <input name="login" id="kc-login" type="submit" class="btn btn-lg btn-primary ...">
</form>
```

**Phase A2 実装での確定セレクタ**:
- `--url "https://moocs.iniad.org/auth/iniad"` (Keycloak へリダイレクトされる)
- `--username-selector "#username"`
- `--password-selector "#password"`
- `--submit-selector "#kc-login"`

---

## 実装進捗 (セッション末尾時点)

### 完了

- ✅ Phase 0: 17 件の実機調査 (Q1〜Q17)
- ✅ Phase A0: setup auto-install + doctor 8 フィールド
- ✅ Phase A1: imoocs-types + imoocs-browser 骨格 (SecretString ベース Credentials, BrowserOps trait, FakeBrowserSession)
- ✅ Phase A2: MOOCs + Google SSO の browser 経由置換 (speedbump 自動 click 込み)
- ✅ Phase A3: keyring 完全撤去、agent-browser auth-vault 一本化
- ✅ Phase A3.5: e2e tests 修正 (旧 keyring/cookies.json 前提を新挙動に追従)
- ✅ Phase B.3: `api::moocs` (course/lesson) を browser 経由に置換 (実機で `imoocs course list` 動作確認済)
- ✅ AssignmentKey 拡張: `lesson_id: Option<String>` / `page_id: Option<String>` 追加 (Phase B.4/B.5 の DOM 抽出経路の URL 解決準備)

### 次セッションで着手予定

#### Phase B.4 / B.5 (DOM 抽出による read 系全面置換)

**スコープ**:
1. `imoocs-browser/src/commands/assignments.rs` 新規:
   - `fetch_assignment_dom(binary, page_url)` で課題ページを navigate
   - 必要なら「問題を開く」ボタンを click (要 visible 判定)
   - `.problem-container` / `.problem-contentpage` の outerHTML + textarea 各 value + assessment テキスト を 1 batch で取得
2. `imoocs-core/src/scrape/assignment_page.rs` 新規:
   - `detect_status(html)` → AssignmentStatus (`.alert.problem-closed-only` で Closed、`.problem-assessment` visible で Graded、`.problem-coverpage` visible で Open など)
   - `detect_network_restricted(html)` → bool (Q5 で発見済の `.alert.problem-closed-only` 文言類推)
   - `extract_answers(html)` → HashMap<String, AnswerEntry> (`<textarea name>` から value 収集)
   - `extract_assessment(html)` → Assessment (`.problem-assessment .panel-primary` から `##/##` パース)
3. `imoocs-core/src/api/assignments.rs` の `get_status` / `get_problem_html` / `get_answers` / `get_assessment` を上記に置換
4. fixture HTML (Q4 で取得した採点エリア構造、Q5 で取得した closed 文言など) を `crates/imoocs-browser/tests/fixtures/` に保存し unit test

**Phase 0 で確定済のセレクタ** (実装着手時にそのまま使える):
- `.problem-container .problem-contentpage .alert.problem-closed-only` (Closed 状態)
- `.problem-container .problem-contentpage .problem-contents` (本文ロード完了判定)
- `.problem-container .problem-contentpage .problem-assessment .panel-primary` (採点表示)
- `button.start-answer` (問題を開く、初期状態で visible なら click)
- `<textarea name="pNN">` (回答 pid)

**URL 解決**: `AssignmentKey { lesson_id, page_id }` を埋めて caller から渡す。両方 Some なら `/courses/<year>/<course>/<lesson>/<page>` で navigate。None なら fallback (内部 API URL navigate or エラー返却)。

#### Phase C / D / E

- Phase C: write 系 (`put_answers` / `post_file` / `get_file`) を browser 経由に。CSRF 削除は Phase C Exit 後の別 commit
- Phase D: Drive (D0 spike → D1 実装 → D2 reconciliation → D3 cleanup), Slides (D0 戦略決定 → D1 実装。戦略 B 確定済? Phase 0 結果は B 不可で戦略 A 採用)
- Phase E: 依存大掃除 (戦略 A 採用なら `lopdf` 残置、`usvg`/`svg2pdf`/`unicode-escape`/`pdf-writer` 削除、`reqwest` 系完全削除)

---

## Phase 0 完了サマリ

| Q | 状態 | 結論 |
|---|---|---|
| Q1 | ✅ 確定 | Keycloak ログイン画面の a11y tree 確定 |
| Q2 | ✅ 確定 | SAML chain は ACS まで自動進行、speedbump で 1 click 必要 (本人確認) |
| Q3 | ✅ 確定 | 課題は `<form>` 無し + JS XHR、`data-urlprefix` で API URL 組み立て |
| Q4 | ✅ 確定 | `.problem-assessment .panel.panel-primary` で採点表示。`##/##` を JS が置換 |
| Q5 | ⚠️ 部分確定 | Closed 状態の `.alert.problem-closed-only` 文言「現在回答を受け付けていません。」確定。NETWORK_RESTRICTED 専用文言は学外環境で要追跡 |
| Q6 | 📝 Phase C で観察 | `.alert-success` 等 Bootstrap 慣例から予測。destructive なので Phase 0 では確定せず |
| Q9 | ✅ 確定 | `batch --json` shape: `[{command, error, result, success}, ...]` |
| Q10 | ✅ 確定 | `<input type=file>` upload には `--allow-file-access` 不要 |
| Q11 | ✅ 確定 | `download <selector> <path>` は selector click 型 |
| Q12 | ✅ 確定 | 戦略 B (公式 /export/pdf) は ❌、戦略 A (per-slide + lopdf) 採用。`lopdf` は残置 |
| Q13 | 📝 Phase D0 で観察 | Google 認証必要なので Phase A2 後の D0 spike で |
| Q15 | ✅ 確定 | reCAPTCHA は出ない (INIAD SAML 経由)。headed fallback はオプション扱い |
| Q16 | ✅ 確定 | `.markdown-block` ではなく `.problem-contents` (or `.alert` or `.problem-assessment`) を待つ |
| Q17 | ✅ 確定 | Keycloak セレクタ `#username` / `#password` / `#kc-login` で auth save/login が動作 |
| pdf | ✅ 確定 | paperSize フラグ無し、`set viewport 1280 720` を pdf 前に叩く |
| eval | ✅ 確定 | top-level await NG、`Promise.then(() => window.flag = true)` + `wait --fn` パターン |
| state | ✅ 確定 | 暗号化は agent-browser が自動 (`~/.agent-browser/.encryption-key`) |
| auth-vault | ✅ 確定 | `~/.agent-browser/auth/<name>.json` AES-256-GCM 自動暗号化 |

### Phase A 着手準備完了

- Phase A0 (auto install + doctor): 着手可
- Phase A1 (骨格 + 共有型): 着手可
- Phase A2 (認証移行): 着手可。実装セレクタも確定済 (`#username` / `#password` / `#kc-login`)

### Phase B 着手準備完了

- Q4 / Q16 確定で本文・採点抽出ロジックの設計が固まった

### Phase C 着手前に残課題

- Q6 (提出後 toast) は Phase C 着手時の最初の destructive test で確定

### Phase D 着手前に残課題

- Q13 (フォント待ち) は Phase A2 完了後 (Google 認証ヘッドレス利用可能になった時点) に D0 spike で確定
