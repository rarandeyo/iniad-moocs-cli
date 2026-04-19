//! Google Workspace SAML login via the INIAD SSO.
//!
//! Adapted from moocs-collect `src/repository/auth.rs:101-185` (MIT, Copyright 2024
//! Yuki Natori) with the unchecked `.unwrap()` calls replaced by `?` that produce
//! typed errors, so we get clean structured exit codes instead of panics.

use once_cell::sync::Lazy;
use regex::Regex;
use scraper::Html;
use tracing::{debug, info};

use crate::auth::moocs::Credentials;
use crate::error::{ImoocsError, Result};
use crate::session::Session;
use crate::util::html::extract_element_attribute;

const SAML_REDIRECT_URL: &str = "https://accounts.google.com/samlredirect?domain=iniad.org";
const INVALID_CREDS_MARKER: &str = "Invalid username or password.";

static ANCHOR_HREF_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"<a\s+(?:[^>]*?\s+)?href="([^"]*)""#).unwrap());
static META_REFRESH_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"<meta\s+http-equiv="refresh"\s+content=".*?\s+url=(.*?)">"#).unwrap()
});

pub async fn login_google(session: &Session, creds: &Credentials) -> Result<()> {
    // Step 1: bootstrap
    let body = session
        .client
        .get(SAML_REDIRECT_URL)
        .send()
        .await?
        .text()
        .await?;

    // Step 2: Keycloak username/password (if shown)
    let mut document = Html::parse_document(&body);
    let initial_action =
        extract_element_attribute(&document.root_element(), "form.form-signin", "action");
    if let Ok(action) = initial_action {
        debug!("submitting Keycloak credentials for Google SAML");
        let post = session
            .client
            .post(&action)
            .form(&[
                ("username", creds.username.as_str()),
                ("password", creds.password.as_str()),
            ])
            .send()
            .await?;
        let post_body = post.text().await?;
        if post_body.contains(INVALID_CREDS_MARKER) {
            return Err(ImoocsError::Auth {
                reason: "invalid username or password (SAML)".into(),
                hint: Some("re-run `imoocs auth login` to update stored credentials".into()),
            });
        }
        document = Html::parse_document(&post_body);
        // Must now have the saml-post-binding form to continue.
        extract_element_attribute(
            &document.root_element(),
            "form[name='saml-post-binding']",
            "action",
        )
        .map_err(|e| ImoocsError::Auth {
            reason: format!("unexpected SAML response after Keycloak login: {e}"),
            hint: Some("INIAD SSO or Google SAML flow may have changed; file an issue".into()),
        })?;
    }

    // Step 3: saml-post-binding → POST to continue SAML assertion
    let (action, saml_response, relay_state) = {
        let root = document.root_element();
        (
            extract_element_attribute(&root, "form[name='saml-post-binding']", "action")?,
            extract_element_attribute(&root, "input[name='SAMLResponse']", "value")?,
            extract_element_attribute(&root, "input[name='RelayState']", "value")?,
        )
    };
    debug!(%action, "posting SAMLResponse to {}", action);
    let resp = session
        .client
        .post(&action)
        .form(&[
            ("SAMLResponse", saml_response.as_str()),
            ("RelayState", relay_state.as_str()),
        ])
        .send()
        .await?;
    let body = resp.text().await?;

    // Step 4: hiddenpost → POST
    let document = Html::parse_document(&body);
    let (action, relay_state, saml_response, trampoline) = {
        let root = document.root_element();
        (
            extract_element_attribute(&root, "form[name='hiddenpost']", "action")?,
            extract_element_attribute(&root, "input[name='RelayState']", "value")?,
            extract_element_attribute(&root, "input[name='SAMLResponse']", "value")?,
            extract_element_attribute(&root, "input[name='trampoline']", "value")?,
        )
    };
    let resp = session
        .client
        .post(&action)
        .form(&[
            ("RelayState", relay_state.as_str()),
            ("SAMLResponse", saml_response.as_str()),
            ("trampoline", trampoline.as_str()),
        ])
        .send()
        .await?;
    let body = resp.text().await?;

    // Step 5: follow anchor href
    let href = ANCHOR_HREF_RE
        .captures(&body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .ok_or_else(|| ImoocsError::Parse("SAML step 5: could not locate <a href>".into()))?;
    let body = session
        .client
        .get(href.replace("&amp;", "&"))
        .send()
        .await?
        .text()
        .await?;

    // Step 6: follow meta refresh
    let url = META_REFRESH_RE
        .captures(&body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .ok_or_else(|| ImoocsError::Parse("SAML step 6: could not locate meta refresh".into()))?;
    session.client.get(url.replace("&amp;", "&")).send().await?;

    if is_logged_in_google(session).await? {
        session.save_cookies()?;
        info!("Google SAML login successful");
        Ok(())
    } else {
        Err(ImoocsError::Auth {
            reason: "Google session check failed after SAML flow".into(),
            hint: Some("Google MFA may be enabled; resolve in a browser and retry".into()),
        })
    }
}

pub async fn is_logged_in_google(session: &Session) -> Result<bool> {
    let resp = session
        .client
        .get("https://myaccount.google.com")
        .send()
        .await?;
    Ok(resp.url().domain() == Some("myaccount.google.com"))
}
