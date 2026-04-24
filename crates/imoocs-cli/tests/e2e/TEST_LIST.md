# `imoocs` e2e テストリスト (35 件)

t-wada 流テストリスト = 「リファクタリング前にテストで囲う」outside-in 安全網。
Phase 2 (unit REFACTOR) の前に **35 件全部 green** にすることが目標。

進捗追跡の単一の真実 (CI 不要のため repo 内 markdown が唯一)。
plan: `/home/rarandeyo/.claude/plans/cli-config-e2e-t-wada-tdd-enchanted-harbor.md`

## グループ別実行順 / opt-in 要件

| グループ | 章 | 並列性 | env / opt-in |
|---|---|---|---|
| G1 静的 | 1, 2, 3, 4.1, 6 | 完全並列可 | 無 |
| G2 confirm stage | 4.2-4.5, 5.1-5.3 | 並列可 | 無 (HTTP 不要) |
| G3 認証必要 read-only | 7 | `#[serial]` | keyring |
| G4 completion install | 8 | 並列可 | 無 (XDG 隔離) |
| G5 destructive | 9 | **直列** | `#[ignore]` + `IMOOCS_E2E_ALLOW_DESTRUCTIVE=1` + `IMOOCS_E2E_PROBLEM_ID` |
| G6 lesson best-effort | 10 | 並列可 | keyring + `IMOOCS_E2E_LESSON_URL` |

---

## 1. 配管・契約 (5 件、副作用なし、env 不要)

- [ ] **1.1** `imoocs --version` → exit 0, stdout が `imoocs ` で始まる (Walking Skeleton)
- [ ] **1.2** `imoocs --help` → exit 0, stdout に 11 サブコマンド名すべて含む (`version`, `doctor`, `auth`, `course`, `lesson`, `assignment`, `slide`, `drive`, `open`, `setup`, `completion`)
- [ ] **1.3** `imoocs unknown-cmd` → exit ≠ 0 (clap), stderr に "unrecognized" 系のエラー
- [ ] **1.4** `imoocs version` → exit 0, JSON envelope `{success:true, data:{name, version}}` (常に JSON、`--format` を無視)
- [ ] **1.5** `imoocs --format invalid version` → exit ≠ 0 (clap value_enum), stderr にエラー

## 2. envelope 契約横断 (4 件)

- [ ] **2.1** 失敗 envelope の stdout が **single JSON object** (`assignment submit` の config 未設定エラーなど) — `serde_json::from_slice` でパース可能を assert
- [ ] **2.2** `auth status --format json` (未ログイン) → **text + exit 2** (`--format json` を無視する契約; cli.rs:30 の doc string 由来)
- [ ] **2.3** `success:true` envelope に `error` キーが**ない** (untagged enum 検証; envelope.rs `SuccessFlag` 由来)
- [ ] **2.4** `success:false` envelope に `data` キーが**ない** (`FailureFlag` 由来)

## 3. doctor / config (4 件、3 件は既存 diagnostics.rs を移植)

- [ ] **3.1** `doctor --format json` (clean XDG) → exit 2 + envelope `success:true` + `data.moocsAuthenticated=false`
- [ ] **3.2** `doctor --format json` で config TOML 不正 (`not = [valid\n`) → exit 5 + `success:false` + "config toml parse error" (移植元: 旧 diagnostics.rs L30)
- [ ] **3.3** `doctor --format json` で course-drive-folders.toml 不正 (`driveRootFolderId = 123\n`) → exit 5 + `success:false` + "course-drive-folders.toml parse error" (移植元: 旧 diagnostics.rs L43)
- [ ] **3.4** `auth status` で config 不正 → exit 5 + stderr に "認証状態確認失敗" + "config toml parse error", stdout に "MOOCs login" を**含まない** (fake summary 抑止) (移植元: 旧 diagnostics.rs L68)

## 4. assignment confirm モード stage 契約 (5 件、Phase 2 で壊れやすい本丸、HTTP 不要)

- [ ] **4.1** config 未設定で `assignment submit C P --data '{}'` → exit 3, envelope `error.code=VALIDATION_ERROR` + message に "config `assignment.confirm` is not set" (副作用 0、env 不要)
- [ ] **4.2** confirm モード + `submit C P --data '{"<pid>":"hello"}'` → exit 0, envelope `data.staged=true, data.submitted=false`, `<XDG_STATE_HOME>/imoocs/drafts/<year>-C-P.json` 存在 + JSON 中の `answers["<pid>"] == "hello"` (Walking Skeleton)
- [ ] **4.3** confirm モード + `submit C P --data @path/to/answer.json` → 4.2 と同等の draft 生成
- [ ] **4.4** confirm モード + `submit C P --data -` (stdin) → 4.2 と同等
- [ ] **4.5** invalid JSON `--data 'not-json'` → exit 3, "invalid JSON in --data"

## 5. assignment push 契約 (3 件、PTY brittleness を考慮し最小限)

> 5.1 は assert_cmd で非 TTY 起動 → push が exit 3 で止まることを確認 (PTY 不要)。
> 5.2-5.3 は PTY 必須 (`#[cfg(target_os = "linux")]` で gate)。

- [ ] **5.1** 非 TTY 実行 (assert_cmd 経由) + draft あり (4.2 で stage したもの) + `assignment push C P` → exit 3, message に "must be run from a TTY" + draft が残存
- [ ] **5.2** PTY + draft 無し + `assignment push C P` → exit 4, "no draft staged"
- [ ] **5.3** PTY + draft あり + プロンプトに `n` 回答 → exit 3, "Push cancelled" + draft 残存
- [ ] (5.4 EOF / 5.5 y は PTY が安定化したら追加候補。`y` 確定は **9.2 destructive で** 実行)

## 6. assignment drafts (3 件、HTTP 不要)

- [ ] **6.1** `drafts list` (空) → exit 0, envelope `data == []`
- [ ] **6.2** confirm submit (4.2) 後に `drafts list` → exit 0, length 1, `data[0]` が `{year, courseId, problemId, answerPids: ["<pid>"], filePids: [], updatedAt, path}` の DraftSummary 形 (schema.md L173-181)
- [ ] **6.3** `drafts clear` (引数 0) → exit 3, message に "requires `--all`" 系

## 7. グローバルオプションの env 上書き (2 件、三角測量、認証必要)

- [ ] **7.1** `imoocs --format json version` と `IMOOCS_FORMAT=json imoocs version` が **同じ stdout** を出す (どちらも JSON envelope)
- [ ] **7.2** `imoocs --year 2099 course list` (実 HTTP、要 keyring) → envelope `success:false` + `error.code` が `API_ERROR` または `NOT_FOUND` (どちらも許容; 2099 は MOOCs に存在しない年)

## 8. completion install (3 件、host shell 非依存に fish で固定)

> bash/zsh の per-shell 検出ロジックは既存 unit `commands/completion.rs:210-237` で
> カバー済み。e2e は host から独立した検証として fish のみ。

- [ ] **8.1** `completion install --shell fish` (TempXdg) → exit 0, `<XDG_CONFIG_HOME>/fish/completions/imoocs.fish` 存在 + 中身に `complete -c imoocs` を含む
- [ ] **8.2** 8.1 直後にもう一度 → exit 0, stderr or stdout に "already up to date" 系
- [ ] **8.3** 8.1 後にファイルを手動編集 ("# manual edit\n") → 3 回目 install → exit 3 ("differs"), `--force` 付きで再実行 → exit 0

## 9. destructive (3 件、`#[ignore]` で gating、本番サーバ書き込み)

> 各テスト本体先頭で `IMOOCS_E2E_ALLOW_DESTRUCTIVE=1` AND `IMOOCS_E2E_PROBLEM_ID` を
> 確認、無ければ `eprintln!("[skip]")` + early return。
> submit value は `unique_marker()` (timestamp_nanos + uuid v4) で実行ごと完全ユニーク。

- [ ] **9.1** `IMOOCS_E2E_USERNAME` + `IMOOCS_E2E_PASSWORD` 経由で `auth login --username X --password-stdin` → exit 0, 続けて `auth status` → exit 0
- [ ] **9.2** confirm モード + `submit C P --data '{"<pid>":"<unique_marker>"}'` で stage → PTY で `push C P` + `y` → exit 0, envelope `data.pushed=true, data.submitted=true`, draft 削除確認 + `assignment show` で `currentValue=="<unique_marker>"` 再確認
- [ ] **9.3** auto モード + `submit C P --data '{"<pid>":"<unique_marker>"}'` 即送信 → exit 0, envelope `data.submitted=true`, `assignment show` で `currentValue` がマーカーと一致

## 10. lesson best-effort 契約 (3 件、`362b402` 起因)

> 10.1 と 10.2 は shape 検証のみ薄め (既存 unit `commands/mod.rs:138-221` の
> `populate_slide_pdfs_records_skipped_and_failed_without_propagating` が
> best-effort 契約をしっかり押さえているため)。10.3 が真の新規価値。

- [ ] **10.1** `lesson show C L` (要 `IMOOCS_E2E_LESSON_URL` から C/L を解決、または env で別途指定) → exit 0, envelope `data` に `lesson` と `assignments` キーが両方存在 (`LessonWithAssignments` shape; schema.md L64-76)
- [ ] **10.2** `lesson show C L --no-fetch-slides` → exit 0, envelope `data.lesson.embeds[*].fetchStatus` が **存在しない** (skip_serializing_if で省略される)
- [ ] **10.3** Google SSO 未ログイン状態 (cookies.json から google session を消す) で `lesson show C L --no-cache` (slide 入りページ) → **exit 0** (best-effort 維持) + `data.lesson.embeds[*].fetchStatus` のうち少なくとも 1 つが `"skipped"`

---

## 完了判定

```bash
# Walking Skeleton (1.1, 2.2, 4.2)
cargo test -p imoocs-cli --test e2e walking_skeleton

# 全 e2e (destructive 抜き)
cargo test -p imoocs-cli --test e2e

# destructive 込み (3 重 opt-in)
IMOOCS_E2E_ALLOW_DESTRUCTIVE=1 \
IMOOCS_E2E_USERNAME=s1f10TESTxxx \
IMOOCS_E2E_PASSWORD=... \
IMOOCS_E2E_PROBLEM_ID=... \
IMOOCS_E2E_LESSON_URL=... \
cargo test -p imoocs-cli --test e2e -- --ignored
```

全 35 件のチェックボックスが埋まったら Phase 1 完了 → Phase 2 (Unit REFACTOR) に進む。
