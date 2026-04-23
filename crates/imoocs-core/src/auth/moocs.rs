//! MOOCs (Keycloak) ログイン。
//!
//! moocs-collect `src/repository/auth.rs:80-99` (MIT, Copyright 2024 Yuki Natori) より転用。
//!
//! Flow:
//! 1. `https://moocs.iniad.org/auth/iniad` を GET (redirect を追う。cookie store 付き)
//! 2. 最終 URL が `/courses` ならすでにログイン済みとみなして終了。
//! 3. そうでなければレスポンス HTML から `form.form-signin[action]` を parse する。
//! 4. その action に対して `username` + `password` を `application/x-www-form-urlencoded` で POST。
//! 5. `/account` を GET し、最終 URL のパスが `/account` であることを確認する。

use scraper::Html;
use tracing::{debug, info};

use crate::error::{ImoocsError, Result};
use crate::session::Session;
use crate::util::html::extract_element_attribute;

#[derive(Debug, Clone)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

const LOGIN_BOOTSTRAP_URL: &str = "https://moocs.iniad.org/auth/iniad";
const AFTER_LOGIN_URL: &str = "https://moocs.iniad.org/courses";
const ACCOUNT_PATH: &str = "/account";

pub async fn login_moocs(session: &Session, creds: &Credentials) -> Result<()> {
    let resp = session.client.get(LOGIN_BOOTSTRAP_URL).send().await?;
    let final_url = resp.url().clone();
    debug!(%final_url, "auth/iniad bootstrap complete");

    if final_url.as_str() == AFTER_LOGIN_URL {
        info!("already logged in; no form submission needed");
        session.save_cookies()?;
        return Ok(());
    }

    let body = resp.text().await?;
    let document = Html::parse_document(&body);
    let action = extract_element_attribute(&document.root_element(), "form.form-signin", "action").map_err(|e| {
        ImoocsError::Auth {
            reason: format!("cannot find Keycloak login form: {e}"),
            hint: Some("the INIAD SSO login page may have changed; file an issue".into()),
        }
    })?;

    debug!(%action, "found Keycloak form.form-signin action");
    let post = session
        .client
        .post(&action)
        .form(&[
            ("username", creds.username.as_str()),
            ("password", creds.password.as_str()),
        ])
        .send()
        .await?;
    let post_url = post.url().clone();
    debug!(%post_url, status = ?post.status(), "credentials POSTed to Keycloak");
    // body を消費して cookie と redirect を確実に反映させる
    let _ = post.text().await?;

    if is_logged_in_moocs(session).await? {
        session.save_cookies()?;
        info!("MOOCs login successful");
        Ok(())
    } else {
        Err(ImoocsError::Auth {
            reason: "invalid username or password".into(),
            hint: Some("double-check credentials and run `imoocs auth login` again".into()),
        })
    }
}

pub async fn is_logged_in_moocs(session: &Session) -> Result<bool> {
    let resp = session
        .client
        .get(crate::session::moocs_url(ACCOUNT_PATH))
        .send()
        .await?;
    Ok(resp.url().path() == ACCOUNT_PATH)
}

pub fn logout_local(session: &Session) -> Result<()> {
    session.clear_cookies()?;
    Ok(())
}
