//! 課題答案送信 (write 系) を agent-browser navigate + form fill + click で実現するラッパ。
//!
//! `docs/agent-browser-migration-notes.md` の Q3 で確定した
//! セレクタ (`.problem-container[data-urlprefix]` / `button.start-answer` /
//! `button.submit-answer` / `button.file-trigger-btn` / `<textarea name="<pid>">`) に
//! 依存する。
//!
//! 注意:
//! - submit/upload は destructive な操作なので、呼び出し側で `confirm` モードと
//!   一致するよう制御する
//! - Q6 (提出後 toast) は実機で確定するまで `wait_fn` を generic セレクタにしておき、
//!   実機で観察して確定後にしぼる

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use crate::batch::{BatchBuilder, BatchResponse};
use crate::error::BrowserError;
use crate::process::AgentBrowser;

/// 課題ページに navigate し、textarea / input[type=text] を fill して
/// `button.submit-answer` を click する。
///
/// 前提として **`imoocs auth login` で agent-browser daemon にも MOOCs session が
/// 確立されている** こと。reqwest 側 cookies と daemon 側 cookies は
/// サーバ的に別の session として扱われるので、reqwest 側 cookie を inject しても
/// 意味がない (むしろ daemon の valid session を上書きしてしまう)。
///
/// `urlprefix` は `.problem-container[data-urlprefix]` の値 (例:
/// `/assignments/2026/INI301/ai-s01-assign1`)。
pub async fn submit_answer(
    binary: &Path,
    page_url: &str,
    urlprefix: &str,
    answers: &HashMap<String, String>,
) -> Result<(), BrowserError> {
    let agent = AgentBrowser::new(binary.to_path_buf(), "imoocs");

    let mut builder = BatchBuilder::new();
    let container_sel_for_js = format!(r#".problem-container[data-urlprefix="{}"]"#, urlprefix);
    builder
        .open(page_url)
        .wait_fn("document.readyState === 'complete'", 30_000)
        .wait_fn(
            format!(r#"document.querySelector('{}') !== null"#, container_sel_for_js),
            30_000,
        );

    // 1. 問題を開く (.start-answer はクリックしても visible でなければ no-op)
    builder.eval(open_problem_js(&container_sel_for_js));
    // 2. contentpage が表示されるまで待つ
    builder.wait_fn(content_ready_js(&container_sel_for_js), 30_000);

    // 3. textarea / input[type=text] を fill (DOM 直接 + input/change イベント発火で
    //    React/jQuery 両方を起こす)
    for (pid, value) in answers {
        builder.eval(fill_value_js(&container_sel_for_js, pid, value));
    }

    // 4. submit ボタン click。`confirm()` dialog 等で Chrome が hang するのを防ぐため、
    //    先に `window.alert / window.confirm` を auto-accept で上書きする。あわせて
    //    XHR/fetch monitor を install して、submit が裏で投げる PUT /answers の完了を
    //    動的に待てるようにする (= 固定 wait_ms より速くて信頼できる)。
    builder.eval(suppress_dialogs_js());
    builder.eval(install_xhr_monitor_js());
    builder.eval(click_submit_js(&container_sel_for_js));

    // 5. XHR が完了 + 200ms idle で全て落ち着いたと判定する。30 秒は安全上限。
    builder.wait_fn(wait_xhr_idle_js(), 30_000);

    // 6. daemon が hang しないよう Chrome instance を閉じる (次回起動で session 復元)
    builder.push(["close"]);

    run_batch(&agent, &builder, "submit_answer").await
}

/// `input[type=file][name=<pid>]` にファイルを upload して `button.submit-answer` を click。
///
/// 内部 `data-urlprefix` から `.file-trigger-btn` 等の存在は確認済 (Q3)。upload は
/// agent-browser の `upload <selector> <path>` を使い、`<input type=file>` に直接 attach する。
pub async fn upload_file(
    binary: &Path,
    page_url: &str,
    urlprefix: &str,
    pid: &str,
    file_path: &Path,
) -> Result<(), BrowserError> {
    // `submit_answer` と同じ前提: daemon Chrome に MOOCs session 確立済。
    let agent = AgentBrowser::new(binary.to_path_buf(), "imoocs");

    let mut builder = BatchBuilder::new();
    let container_sel_for_js = format!(r#".problem-container[data-urlprefix="{}"]"#, urlprefix);
    let file_input_css = format!(
        r#".problem-container[data-urlprefix="{}"] input[type="file"][name="{}"]"#,
        urlprefix, pid
    );

    builder
        .open(page_url)
        .wait_fn("document.readyState === 'complete'", 30_000)
        .wait_fn(
            format!(r#"document.querySelector('{}') !== null"#, container_sel_for_js),
            30_000,
        );
    builder.eval(open_problem_js(&container_sel_for_js));
    builder.wait_fn(content_ready_js(&container_sel_for_js), 30_000);

    // `upload <css> <path>` で file input に attach (Q10 で `--allow-file-access` 不要と確認)
    builder.push(["upload".to_string(), file_input_css, file_path.display().to_string()]);
    builder.eval(suppress_dialogs_js());
    builder.eval(install_xhr_monitor_js());
    builder.eval(click_submit_js(&container_sel_for_js));
    // XHR (POST /file/<pid>) を XHR monitor で動的に待つ。file 本体サイズ依存なので
    // 30 秒の安全上限を確保 (回線速度が遅くて大きい file のとき効く)。
    builder.wait_fn(wait_xhr_idle_js(), 30_000);
    builder.push(["close"]);

    run_batch(&agent, &builder, "upload_file").await
}

fn suppress_dialogs_js() -> String {
    // submit が裏で `confirm("提出しますか?")` 等を投げると Chrome が応答待ちで
    // hang するため、auto-accept する no-op に差し替える。MOOCs JS の挙動は Q6
    // で確定するまで未確認なので、両方とも auto-accept しておく。
    "window.alert=function(){};window.confirm=function(){return true;};1".to_string()
}

/// XMLHttpRequest と fetch を hook して進行中リクエスト数 + 最後の完了時刻を
/// `window.__moocsActiveReqs` / `window.__moocsLastFinish` に記録する。
///
/// 呼び出すべきタイミングは **submit click の直前**。それより前に install すると
/// start-answer click の問題内容取得 XHR も counter に乗ってしまい誤検知する。
/// 既に install 済みなら再 install しない (idempotent)。
fn install_xhr_monitor_js() -> String {
    r#"(function(){if(window.__moocsActiveReqs!==undefined)return;window.__moocsActiveReqs=0;window.__moocsLastFinish=0;var origSend=XMLHttpRequest.prototype.send;XMLHttpRequest.prototype.send=function(){window.__moocsActiveReqs++;this.addEventListener('loadend',function(){window.__moocsActiveReqs--;window.__moocsLastFinish=Date.now();});return origSend.apply(this,arguments);};if(window.fetch){var origFetch=window.fetch;window.fetch=function(){window.__moocsActiveReqs++;return origFetch.apply(this,arguments).finally(function(){window.__moocsActiveReqs--;window.__moocsLastFinish=Date.now();});};}})();1"#.to_string()
}

/// 「進行中リクエスト 0 件 + 最後の完了から 200ms 経過」で true。
/// monitor 未 install (= flag 未定義) なら false (= 念のため timeout まで待つ)。
fn wait_xhr_idle_js() -> String {
    r#"(function(){if(typeof window.__moocsActiveReqs==='undefined')return false;if(window.__moocsActiveReqs>0)return false;var lf=window.__moocsLastFinish||0;return lf>0&&(Date.now()-lf>200);})()"#.to_string()
}

async fn run_batch(agent: &AgentBrowser, builder: &BatchBuilder, op: &str) -> Result<(), BrowserError> {
    let json = builder.to_json().map_err(BrowserError::from)?;
    let value: Value = agent.run_raw(&["batch"], Some(json.as_bytes())).await?;
    let outcomes: BatchResponse = serde_json::from_value(value)?;
    for (i, o) in outcomes.iter().enumerate() {
        if !o.success {
            return Err(BrowserError::CommandFailed(format!(
                "{op}: command #{i} ({:?}) failed: {}",
                o.command,
                o.error.as_deref().unwrap_or("unknown")
            )));
        }
    }
    Ok(())
}

fn open_problem_js(container_sel: &str) -> String {
    format!(
        "(function(){{var c=document.querySelector({sel});if(!c)return;var b=c.querySelector('button.start-answer');if(b&&b.offsetParent!==null){{b.click();}}}})();1",
        sel = js_string(container_sel)
    )
}

fn content_ready_js(container_sel: &str) -> String {
    format!(
        "(function(){{var c=document.querySelector({sel});if(!c)return false;var pp=c.querySelector('.problem-contentpage');return pp && pp.offsetParent !== null;}})()",
        sel = js_string(container_sel)
    )
}

fn fill_value_js(container_sel: &str, pid: &str, value: &str) -> String {
    // textarea[name=<pid>] と input[type=text][name=<pid>] の両方に対応。
    // CSS selector の attribute value は引用で囲って pid に特殊文字が来ても安全にする。
    format!(
        "(function(){{var c=document.querySelector({csel});if(!c)return;var pid={pid};var el=c.querySelector('textarea[name=\"'+pid+'\"]')||c.querySelector('input[type=\"text\"][name=\"'+pid+'\"]');if(!el)return;el.value={val};el.dispatchEvent(new Event('input',{{bubbles:true}}));el.dispatchEvent(new Event('change',{{bubbles:true}}));}})();1",
        csel = js_string(container_sel),
        pid = js_string(pid),
        val = js_string(value)
    )
}

fn click_submit_js(container_sel: &str) -> String {
    format!(
        "(function(){{var c=document.querySelector({sel});if(!c)return;var b=c.querySelector('button.submit-answer');if(b){{b.click();}}}})();1",
        sel = js_string(container_sel)
    )
}

// success_toast_js: 削除 (CDP timeout 問題で wait_fn 信頼性が低いため未使用)。
// 結果検証は呼び出し側で reqwest `/status` / `/answers` を叩く方式に切り替えた。

/// JS の文字列リテラルを安全に組み立てる。`'`, `"`, `\`, 改行を escape。
fn js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_string_escapes_quote_backslash_newline() {
        assert_eq!(js_string("hello"), r#""hello""#);
        assert_eq!(js_string(r#"a"b"#), r#""a\"b""#);
        assert_eq!(js_string("a\\b"), r#""a\\b""#);
        assert_eq!(js_string("a\nb"), r#""a\nb""#);
        assert_eq!(js_string(""), r#""""#);
    }

    #[test]
    fn fill_value_js_includes_all_args_and_handles_both_input_types() {
        let js = fill_value_js(
            r#".problem-container[data-urlprefix="/assignments/2026/INI301/ai-s01-assign1"]"#,
            "p01",
            "hello\nworld",
        );
        assert!(js.contains("p01"));
        assert!(js.contains("hello"));
        assert!(js.contains("\\n"), "newline should be escaped: {js}");
        assert!(js.contains("textarea"), "should target textarea: {js}");
        assert!(js.contains("input"), "should also target text input: {js}");
    }

    #[test]
    fn open_problem_js_uses_start_answer_class() {
        let js = open_problem_js(r#".problem-container[data-urlprefix="/x"]"#);
        assert!(js.contains("start-answer"));
        assert!(js.contains("offsetParent"));
    }

    #[test]
    fn click_submit_js_uses_submit_answer_class() {
        let js = click_submit_js(r#".problem-container[data-urlprefix="/x"]"#);
        assert!(js.contains("submit-answer"));
    }
}
