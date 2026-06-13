//! MOOCs (Keycloak) ログイン — agent-browser auth-vault 経由。
//!
//! Phase A2.1 で実装。Phase 0 Q17 で確定したセレクタを使う:
//! - `--url "https://moocs.iniad.org/auth/iniad"` (Keycloak へリダイレクトされる)
//! - `--username-selector "#username"`
//! - `--password-selector "#password"`
//! - `--submit-selector "#kc-login"` (実体は `<input type="submit">`)
//!
//! 実装の安全要件:
//! - password は `SecretString` で受け取り、子プロセス stdin への書き込み完了後に
//!   呼び出し側で `zeroize` される (`Credentials::Drop` 実装)
//! - shell コマンド組み立てではなく Rust から直接 stdin 書き込み

use std::path::Path;

use imoocs_types::Credentials;
use serde_json::Value;

use crate::error::BrowserError;
use crate::process::AgentBrowser;

/// agent-browser auth profile の name。imoocs では `moocs` 固定。
pub const PROFILE_NAME: &str = "moocs";

/// MOOCs ログインの URL とセレクタ (Phase 0 Q17 で確定)。
const LOGIN_URL: &str = "https://moocs.iniad.org/auth/iniad";
const USERNAME_SELECTOR: &str = "#username";
const PASSWORD_SELECTOR: &str = "#password";
const SUBMIT_SELECTOR: &str = "#kc-login";

/// 後ログイン確認用 URL とパス (Phase 0 Q1 で確認した挙動と同じ)。
const ACCOUNT_URL: &str = "https://moocs.iniad.org/account";
const ACCOUNT_PATH: &str = "/account";

/// `agent-browser auth save moocs` で credentials を auth-vault に保存する。
///
/// stdin への password 書き込みは `AgentBrowser::run_with_stdin` 経由 (Rust から直接、shell を経由しない)。
///
/// 内部で **既存の `imoocs` session を先に state clear する**。Keycloak ページに行かずに
/// 既ログイン Cookie で /courses 直行 → セレクタ見つからずエラーになる挙動を回避するため。
async fn save_profile(binary: &Path, creds: &Credentials) -> Result<(), BrowserError> {
    let agent = AgentBrowser::new(binary.to_path_buf(), "imoocs");
    // 既存 session を消す (冪等、失敗しても無視)。
    // **agent-browser daemon のメモリ内 Chrome cookie もリセット**するため
    // `state clear` (永続ファイル) + `close` (Chrome instance) の両方を呼ぶ。
    let _ = agent.run(&["state", "clear", "imoocs", "--all"]).await;
    let _ = agent.run(&["close"]).await;
    let args = [
        "auth",
        "save",
        PROFILE_NAME,
        "--url",
        LOGIN_URL,
        "--username",
        creds.username.as_str(),
        "--password-stdin",
        "--username-selector",
        USERNAME_SELECTOR,
        "--password-selector",
        PASSWORD_SELECTOR,
        "--submit-selector",
        SUBMIT_SELECTOR,
    ];
    let password_bytes = creds.password().as_bytes().to_vec();
    let _value: Value = agent
        .run_with_stdin(&args, Some(&password_bytes))
        .await?;
    // password_bytes は明示的に zeroize する (Vec<u8> なので Drop だけでは消えない)
    let mut zeroize_buf = password_bytes;
    use zeroize::Zeroize;
    zeroize_buf.zeroize();
    Ok(())
}

/// `agent-browser auth login moocs` で実ログインを実行する。
///
/// auth-vault に profile が保存済みなら credentials 不要で daemon 単独で再ログイン
/// できる (daemon 再起動で session が全損したときの回復経路として `auth_google::
/// ensure_google_session` からも使う)。
pub(crate) async fn login_with_profile(binary: &Path) -> Result<(), BrowserError> {
    let agent = AgentBrowser::new(binary.to_path_buf(), "imoocs");
    let _value: Value = agent.run(&["auth", "login", PROFILE_NAME]).await?;
    Ok(())
}

/// MOOCs ログインのフルフロー: save profile → login → /account で確認 → 明示的に state save。
///
/// 注意:
/// - daemon の `close` は呼ばない。close すると次の login-google で MOOCs session cookie
///   が daemon メモリから消えて SAML chain が iniad ログイン画面に戻る
/// - 代わりに `state save` を明示的に呼んで永続化する。次回起動時の `--session-name imoocs`
///   による auto-restore で同じ cookie が復元される
pub async fn login_moocs(binary: &Path, creds: &Credentials) -> Result<(), BrowserError> {
    save_profile(binary, creds).await?;
    login_with_profile(binary).await?;
    if !is_logged_in_moocs(binary).await? {
        return Err(BrowserError::CommandFailed(
            "MOOCs login: credentials saved but session not established".into(),
        ));
    }
    persist_session(binary).await?;
    Ok(())
}

/// `--session-name imoocs` で auto-restore される場所に state を明示的に save する。
pub(crate) async fn persist_session(binary: &Path) -> Result<(), BrowserError> {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    let sessions_dir = home.join(".agent-browser").join("sessions");
    let _ = std::fs::create_dir_all(&sessions_dir);
    let target = sessions_dir.join("imoocs-default.json");
    let agent = AgentBrowser::new(binary.to_path_buf(), "imoocs");
    let _ = agent
        .run(&["state", "save", target.to_str().unwrap_or("")])
        .await;
    Ok(())
}

/// `/account` を navigate して URL が `/account` のままなら logged in 判定。
pub async fn is_logged_in_moocs(binary: &Path) -> Result<bool, BrowserError> {
    let agent = AgentBrowser::new(binary.to_path_buf(), "imoocs");
    let _ = agent.run(&["open", ACCOUNT_URL]).await?;
    let value: Value = agent.run(&["get", "url"]).await?;
    let final_url = value
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    Ok(final_url.contains(ACCOUNT_PATH))
}

/// `imoocs auth logout` 用。auth profile を削除 + session state を全消去。
pub async fn logout(binary: &Path) -> Result<(), BrowserError> {
    let agent = AgentBrowser::new(binary.to_path_buf(), "imoocs");
    // profile が存在しない場合の auth delete は agent-browser が success: false を返す。
    // logout は冪等にしたいので、エラーは握りつぶす。
    let _ = agent.run(&["auth", "delete", PROFILE_NAME]).await;
    let _ = agent.run(&["state", "clear", "imoocs", "--all"]).await;
    Ok(())
}
