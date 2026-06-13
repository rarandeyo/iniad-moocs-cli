//! Google Workspace SSO (SAML 経由) login — agent-browser daemon に委譲する。
//!
//! 旧 reqwest SAML 6段 chain は削除。Chrome daemon が同じ
//! `--session-name imoocs` を共有しているので、MOOCs 側 Keycloak セッションが
//! 確立済みなら SAML chain が auto-submit form で自動進行し、speedbump
//! (本人確認ダイアログ) は browser 側で自動 click される。
//!
//! `Session` / `Credentials` の引数は caller 互換のために残しているが、
//! 実体は使用しない。daemon が auth-vault の MOOCs 資格情報を直接使う。

use crate::auth::moocs::Credentials;
use crate::error::Result;
use crate::session::Session;

pub async fn login_google(_session: &Session, _creds: &Credentials) -> Result<()> {
    let binary = crate::api::agent_binary()?;
    imoocs_browser::commands::auth_google::login_google(&binary)
        .await
        .map_err(crate::api::map_browser_err)
}

/// Google にログイン済みなら `Ok(true)`。binary 不在 / ネットワーク失敗 / daemon エラーは
/// すべて `Ok(false)` 扱いにする (doctor / status が clean な状態でも success envelope を
/// 出せるようにするため。Err はあくまで「想定外」のとき)。
pub async fn is_logged_in_google(_session: &Session) -> Result<bool> {
    let Ok(binary) = crate::api::agent_binary() else {
        return Ok(false);
    };
    match imoocs_browser::commands::auth_google::is_logged_in_google(&binary).await {
        Ok(b) => Ok(b),
        Err(e) => {
            tracing::warn!(error = ?e, "is_logged_in_google: browser check failed, treating as not-logged-in");
            Ok(false)
        }
    }
}
