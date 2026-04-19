use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use dialoguer::{Input, Password};
use imoocs_core::{
    auth::{is_logged_in_moocs, login_moocs, Credentials},
    config::Config,
    envelope::ErrorDetail,
    keyring,
    paths::Paths,
    session::Session,
    ImoocsError,
};
use serde::Serialize;
use schemars::JsonSchema;
use serde_json::json;
use tracing::info;

use crate::cli::GlobalArgs;
use crate::output;

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Log in to MOOCs (Keycloak). Prompts for username once; password stored in OS keyring.
    Login {
        /// Override the username (otherwise read from config or prompted).
        #[arg(long, env = "IMOOCS_USERNAME")]
        username: Option<String>,
        /// Read the password from stdin (for CI / agents). Overrides keyring.
        #[arg(long)]
        password_stdin: bool,
    },
    /// Forget stored credentials and cookies.
    Logout {
        /// Keep config.toml (only remove keyring + cookies.json).
        #[arg(long)]
        keep_config: bool,
    },
    /// Report authentication state as JSON.
    Status,
    /// Print stored username (and keyring status). Useful for debugging.
    Export {
        /// Include the stored password in plaintext (DO NOT commit output).
        #[arg(long)]
        unmasked: bool,
    },
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct AuthStatus {
    moocs_authenticated: bool,
    username: Option<String>,
    has_stored_password: bool,
    cookies_path: std::path::PathBuf,
    config_path: std::path::PathBuf,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct AuthExport {
    username: Option<String>,
    password: Option<String>,
    note: Option<String>,
}

pub async fn run(global: &GlobalArgs, cmd: AuthCommand) -> Result<ExitCode> {
    match cmd {
        AuthCommand::Login { username, password_stdin } => login(global, username, password_stdin).await,
        AuthCommand::Logout { keep_config } => logout(global, keep_config).await,
        AuthCommand::Status => status(global).await,
        AuthCommand::Export { unmasked } => export(global, unmasked).await,
    }
}

async fn login(
    global: &GlobalArgs,
    username_arg: Option<String>,
    password_stdin: bool,
) -> Result<ExitCode> {
    use std::io::Read;

    let paths = Paths::discover()?;
    let mut cfg = Config::load(&paths.config_file())?;

    // Username: prefer --username flag > config > prompt.
    let username = match username_arg.or(cfg.username.clone()) {
        Some(u) => u,
        None => Input::<String>::new()
            .with_prompt("INIAD username (e.g. s1f10XXXXXXX)")
            .interact_text()?,
    };

    // Password: --password-stdin > keyring > prompt.
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
                let p: String = Password::new().with_prompt("INIAD password").interact()?;
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
            output::emit_success(
                json!({ "authenticated": true, "username": username }),
                global.format,
            );
            Ok(ExitCode::from(0))
        }
        Err(err @ ImoocsError::Auth { .. }) => {
            // Wipe stored password so the next attempt re-prompts.
            let _ = keyring::delete_credential(&username);
            output::emit_failure::<serde_json::Value>(&ErrorDetail::from_error(&err));
            Ok(ExitCode::from(err.exit_code().as_u8()))
        }
        Err(err) => {
            output::emit_failure::<serde_json::Value>(&ErrorDetail::from_error(&err));
            Ok(ExitCode::from(err.exit_code().as_u8()))
        }
    }
}

async fn logout(global: &GlobalArgs, keep_config: bool) -> Result<ExitCode> {
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

    output::emit_success(json!({ "loggedOut": true }), global.format);
    let _ = global; // silence unused warning when emit_success above uses it
    Ok(ExitCode::from(0))
}

async fn status(global: &GlobalArgs) -> Result<ExitCode> {
    let paths = Paths::discover()?;
    let cfg = Config::load(&paths.config_file())?;
    let session = Session::new(paths.clone_paths())?;

    let moocs_auth = is_logged_in_moocs(&session).await.unwrap_or(false);
    let has_pw = cfg
        .username
        .as_deref()
        .map(|u| keyring::get_password(u).ok().flatten().is_some())
        .unwrap_or(false);

    let data = AuthStatus {
        moocs_authenticated: moocs_auth,
        username: cfg.username,
        has_stored_password: has_pw,
        cookies_path: paths.cookies_file(),
        config_path: paths.config_file(),
    };
    output::emit_success(data, global.format);
    Ok(ExitCode::from(if moocs_auth { 0 } else { 2 }))
}

async fn export(global: &GlobalArgs, unmasked: bool) -> Result<ExitCode> {
    let paths = Paths::discover()?;
    let cfg = Config::load(&paths.config_file())?;

    let username = cfg.username.clone();
    let password = username.as_deref().and_then(|u| keyring::get_password(u).ok().flatten());

    let (password_field, note) = match (password, unmasked) {
        (Some(p), true) => (Some(p), Some("password printed unmasked".into())),
        (Some(p), false) => (Some(mask(&p)), Some("password masked; re-run with --unmasked to show".into())),
        (None, _) => (None, Some("no password stored in OS keyring".into())),
    };

    output::emit_success(
        AuthExport {
            username,
            password: password_field,
            note,
        },
        global.format,
    );
    Ok(ExitCode::from(0))
}

fn mask(s: &str) -> String {
    if s.len() <= 4 {
        "***".into()
    } else {
        format!("{}***{}", &s[..2], &s[s.len() - 2..])
    }
}
