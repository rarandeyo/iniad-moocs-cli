use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use dialoguer::{Input, Password};
use imoocs_core::{
    auth::{is_logged_in_google, is_logged_in_moocs, login_google, login_moocs, Credentials},
    config::Config,
    keyring,
    paths::Paths,
    session::Session,
    ImoocsError,
};
use tracing::info;

use crate::cli::GlobalArgs;

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// MOOCs (Keycloak) にログインする。初回のみ username を聞き、password は OS の keyring に保存。
    Login {
        /// username を上書きする (未指定時は config から読むかプロンプト)。
        #[arg(long, env = "IMOOCS_USERNAME")]
        username: Option<String>,
        /// stdin から password を読む (CI / agent 向け)。keyring より優先。
        #[arg(long)]
        password_stdin: bool,
    },
    /// Google Workspace (INIAD SSO 経由の SAML) にログインする。スライド PDF 取得に必要。
    LoginGoogle,
    /// 保存済みの credential と cookie を破棄する。
    Logout {
        /// config.toml を残す (keyring + cookies.json のみ削除)。
        #[arg(long)]
        keep_config: bool,
    },
    /// 認証状態を報告する。MOOCs にログイン済みなら exit 0、未ログインなら exit 2。
    Status,
    /// 保存済みの username と keyring entry の有無を表示する。
    /// password 自体は出力されない — 必要なら OS の keyring
    /// (macOS Keychain / GNOME Keyring / Windows Credential Manager) を直接参照する。
    Export,
}

pub async fn run(_global: &GlobalArgs, cmd: AuthCommand) -> Result<ExitCode> {
    match cmd {
        AuthCommand::Login {
            username,
            password_stdin,
        } => login(username, password_stdin).await,
        AuthCommand::LoginGoogle => login_google_cmd().await,
        AuthCommand::Logout { keep_config } => logout(keep_config).await,
        AuthCommand::Status => status().await,
        AuthCommand::Export => export().await,
    }
}

/// `auth login` の business logic。envelope emit を含まず `ImoocsError` を
/// そのまま返すので、`imoocs setup` 等のファサードから再利用できる。
#[derive(Debug)]
pub struct LoginOutcome {
    pub username: String,
}

pub async fn do_login(
    username_arg: Option<String>,
    password_stdin: bool,
) -> std::result::Result<LoginOutcome, ImoocsError> {
    use std::io::Read;

    let paths = Paths::discover()?;
    let mut cfg = Config::load(&paths.config_file())?;

    let username = match username_arg.or(cfg.username.clone()) {
        Some(u) => u,
        None => Input::<String>::new()
            .with_prompt("INIAD username (e.g. s1f10XXXXXXX)")
            .interact_text()
            .map_err(map_dialoguer_err)?,
    };

    let password = if password_stdin {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        let p = buf.trim_end_matches(['\r', '\n']).to_string();
        keyring::set_password(&username, &p)?;
        p
    } else {
        match keyring::get_password(&username)? {
            Some(p) => p,
            None => {
                let p: String = Password::new()
                    .with_prompt("INIAD password")
                    .interact()
                    .map_err(map_dialoguer_err)?;
                keyring::set_password(&username, &p)?;
                p
            }
        }
    };

    let session = Session::new(paths.clone_paths())?;
    let creds = Credentials {
        username: username.clone(),
        password,
    };

    match login_moocs(&session, &creds).await {
        Ok(()) => {
            cfg.username = Some(username.clone());
            cfg.save(&paths.config_file())?;
            info!("authenticated as {}", username);
            Ok(LoginOutcome { username })
        }
        Err(err @ ImoocsError::Auth { .. }) => {
            // 次回再プロンプトさせるため、保存済みの password を消す
            let _ = keyring::delete_credential(&username);
            Err(err)
        }
        Err(err) => Err(err),
    }
}

/// `auth login-google` の business logic。成功時は SAML セッションが確立した
/// 対象 username を返す。
pub async fn do_login_google() -> std::result::Result<String, ImoocsError> {
    let paths = Paths::discover()?;
    let cfg = Config::load(&paths.config_file())?;
    let username = cfg
        .username
        .clone()
        .ok_or_else(|| ImoocsError::Validation("no stored username; run `imoocs auth login` first".into()))?;
    let password = keyring::get_password(&username)?
        .ok_or_else(|| ImoocsError::Validation("no password in keyring; run `imoocs auth login` first".into()))?;

    let session = Session::new(paths.clone_paths())?;
    let creds = Credentials {
        username: username.clone(),
        password,
    };
    login_google(&session, &creds).await?;
    Ok(username)
}

async fn login_google_cmd() -> Result<ExitCode> {
    match do_login_google().await {
        Ok(username) => {
            println!("Google SSO session established for {username}.");
            Ok(ExitCode::from(0))
        }
        Err(err) => {
            eprintln!("✗ Google SSO 失敗: {err}");
            Ok(ExitCode::from(err.exit_code().as_u8()))
        }
    }
}

async fn login(username_arg: Option<String>, password_stdin: bool) -> Result<ExitCode> {
    match do_login(username_arg, password_stdin).await {
        Ok(outcome) => {
            println!("Logged in as {}.", outcome.username);
            Ok(ExitCode::from(0))
        }
        Err(err) => {
            eprintln!("✗ ログイン失敗: {err}");
            Ok(ExitCode::from(err.exit_code().as_u8()))
        }
    }
}

async fn logout(keep_config: bool) -> Result<ExitCode> {
    let paths = Paths::discover()?;
    let cfg = Config::load(&paths.config_file())?;

    if let Some(u) = cfg.username.as_deref() {
        let _ = keyring::delete_credential(u);
    }

    let session = Session::new(paths.clone_paths())?;
    let _ = session.clear_cookies();

    if !keep_config {
        Config::clear(&paths.config_file())?;
    }

    println!("Logged out. keyring and cookies cleared.");
    Ok(ExitCode::from(0))
}

async fn status() -> Result<ExitCode> {
    let paths = Paths::discover()?;
    let cfg = Config::load(&paths.config_file())?;
    let session = Session::new(paths.clone_paths())?;

    let moocs_auth = is_logged_in_moocs(&session).await.unwrap_or(false);
    let google_auth = is_logged_in_google(&session).await.unwrap_or(false);
    let has_pw = cfg
        .username
        .as_deref()
        .map(|u| keyring::get_password(u).ok().flatten().is_some())
        .unwrap_or(false);

    let user = cfg.username.as_deref().unwrap_or("-");
    println!("  {} MOOCs login        ({user})", mark(moocs_auth));
    println!("  {} Google SSO", mark(google_auth));
    println!("  {} password stored in keyring", mark(has_pw));
    println!("Paths");
    println!("  cookies  {}", paths.cookies_file().display());
    println!("  config   {}", paths.config_file().display());
    Ok(ExitCode::from(if moocs_auth { 0 } else { 2 }))
}

async fn export() -> Result<ExitCode> {
    let paths = Paths::discover()?;
    let cfg = Config::load(&paths.config_file())?;

    let user = cfg.username.as_deref().unwrap_or("-");
    let has_stored_password = cfg
        .username
        .as_deref()
        .map(|u| keyring::get_password(u).ok().flatten().is_some())
        .unwrap_or(false);

    println!("username: {user}");
    let pw_line = if has_stored_password {
        "stored in OS keyring (never printed by this CLI)"
    } else {
        "not stored"
    };
    println!("password: {pw_line}");
    Ok(ExitCode::from(0))
}

fn mark(ok: bool) -> &'static str {
    if ok {
        "✓"
    } else {
        "✗"
    }
}

pub(crate) fn map_dialoguer_err(e: dialoguer::Error) -> ImoocsError {
    ImoocsError::Internal(format!("prompt failed: {e}"))
}
