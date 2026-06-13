//! Google SSO (SAML 経由) ログイン — agent-browser ヘッドレス + speedbump 自動 click。
//!
//! Phase 0 で実機検証で確定したフロー:
//! 1. `https://accounts.google.com/samlredirect?domain=iniad.org` を navigate
//! 2. SAML chain は ACS まで auto-submit form で自動進行
//! 3. `accounts.google.com/speedbump/samlconfirmaccount` に着地 (初回本人確認ダイアログ)
//! 4. snapshot から `button "続行"` を探して click
//! 5. `myaccount.google.com` ドメインへの到達を確認
//!
//! 前提: 事前に `auth_moocs::login_moocs` で MOOCs (Keycloak) セッションが確立済み。
//! 同じ `--session-name imoocs` を共有しているので Cookie が自動で乗る。

use std::path::Path;

use serde_json::Value;

use crate::error::BrowserError;
use crate::process::AgentBrowser;
use crate::snapshot::Snapshot;

const SAML_REDIRECT_URL: &str = "https://accounts.google.com/samlredirect?domain=iniad.org";
const MYACCOUNT_DOMAIN: &str = "myaccount.google.com";
const SPEEDBUMP_FRAGMENT: &str = "speedbump/samlconfirmaccount";
/// speedbump ページの「続行」ボタンに表示される label。
const CONTINUE_BUTTON_NAME: &str = "続行";

/// SAML chain を通って Google セッションを確立する。
///
/// speedbump (本人確認ダイアログ) に当たった場合は自動で `続行` を click する。
/// reCAPTCHA / 2FA challenge を検出した場合は `BrowserError::ChallengeRequired` を返す
/// (将来 headed fallback で対処)。
pub async fn login_google(binary: &Path) -> Result<(), BrowserError> {
    let agent = AgentBrowser::new(binary.to_path_buf(), "imoocs");

    // Step 1: SAML chain 開始
    agent.run(&["open", SAML_REDIRECT_URL]).await?;
    agent.run(&["wait", "--load", "networkidle", "--timeout", "30000"]).await?;

    // Step 2: 現在 URL を確認
    let url = current_url(&agent).await?;
    tracing::debug!(target: "imoocs_browser::auth_google", url = %url, "after SAML redirect");

    // Step 3: speedbump にいたら 続行 を click
    if url.contains(SPEEDBUMP_FRAGMENT) {
        tracing::info!(target: "imoocs_browser::auth_google", "speedbump detected, clicking 続行");
        let snap: Snapshot = agent.run_json(&["snapshot", "-i"]).await?;
        let (ref_id, _) = snap
            .find_by_name_contains(CONTINUE_BUTTON_NAME)
            .ok_or_else(|| {
                BrowserError::CommandFailed(format!(
                    "speedbump page does not have a `{CONTINUE_BUTTON_NAME}` button"
                ))
            })?;
        let token = format!("@{ref_id}");
        agent.run(&["click", &token]).await?;
        agent
            .run(&["wait", "--load", "networkidle", "--timeout", "30000"])
            .await?;
    }

    // Step 4: 最終的に myaccount.google.com に着地したか確認
    let final_url = current_url(&agent).await?;
    if !final_url.contains(MYACCOUNT_DOMAIN) {
        // challenge or 2FA に飛んでいる可能性
        if final_url.contains("challenge") || final_url.contains("signin/v2") {
            return Err(BrowserError::ChallengeRequired {
                current_url: final_url,
            });
        }
        return Err(BrowserError::CommandFailed(format!(
            "Google SAML did not complete, ended at {final_url}"
        )));
    }

    // 明示的に state save で永続化 (auth_moocs と同じパス)。
    crate::commands::auth_moocs::persist_session(binary).await?;
    Ok(())
}

/// Google にログイン済みか確認。`myaccount.google.com` に navigate して
/// 最終 URL が `myaccount.google.com` ドメインなら true。
pub async fn is_logged_in_google(binary: &Path) -> Result<bool, BrowserError> {
    let agent = AgentBrowser::new(binary.to_path_buf(), "imoocs");
    agent.run(&["open", "https://myaccount.google.com"]).await?;
    let url = current_url(&agent).await?;
    Ok(url.contains(MYACCOUNT_DOMAIN))
}

/// Google session を保証する。切れていたら自動回復を試みる。
///
/// daemon が再起動すると Google session は cookie restore では復活しない
/// (Google 側の device binding。Phase D-2.x 実機検証で確定)。回復チェーン:
/// 1. `myaccount.google.com` 到達確認 → 生きていれば即 return
/// 2. MOOCs (Keycloak) session が切れていたら auth-vault の `moocs` profile で
///    daemon 単独再ログイン (credentials 不要)
/// 3. SAML chain (`login_google`) で Google session を再確立 (speedbump auto-click 込み)
pub async fn ensure_google_session(binary: &Path) -> Result<(), BrowserError> {
    if is_logged_in_google(binary).await? {
        return Ok(());
    }
    tracing::info!(target: "imoocs_browser::auth_google", "Google session expired; attempting recovery");
    if !crate::commands::auth_moocs::is_logged_in_moocs(binary).await? {
        tracing::info!(target: "imoocs_browser::auth_google", "MOOCs session also expired; re-login via auth-vault profile");
        crate::commands::auth_moocs::login_with_profile(binary).await?;
    }
    login_google(binary).await
}

async fn current_url(agent: &AgentBrowser) -> Result<String, BrowserError> {
    let value: Value = agent.run(&["get", "url"]).await?;
    Ok(value
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string())
}
