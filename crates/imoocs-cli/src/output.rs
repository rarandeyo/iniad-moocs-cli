use std::io::{self, Write};

use clap::ValueEnum;
use imoocs_core::envelope::{Envelope, ErrorDetail};
use schemars::JsonSchema;
use serde::Serialize;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
#[value(rename_all = "lowercase")]
pub enum OutputMode {
    Json,
    Pretty,
    Ndjson,
}

impl OutputMode {
    pub fn is_json(self) -> bool {
        matches!(self, OutputMode::Json | OutputMode::Ndjson)
    }
}

pub fn init_tracing(debug: bool, quiet: bool) {
    let level = if debug {
        "imoocs=debug,imoocs_core=debug"
    } else if quiet {
        "imoocs=error,imoocs_core=error"
    } else {
        "imoocs=info,imoocs_core=info"
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    let _ = tracing_subscriber::registry()
        .with(fmt::layer().json().with_writer(io::stderr).with_ansi(false))
        .with(filter)
        .try_init();
}

pub fn emit_success<T: Serialize + JsonSchema>(data: T, mode: OutputMode) {
    let env: Envelope<T> = Envelope::success(data);
    write_envelope(&env, mode);
}

pub fn emit_failure<T: Serialize + JsonSchema>(err: &ErrorDetail) {
    let env: Envelope<T> = Envelope::failure(err.clone());
    write_envelope(&env, OutputMode::Json);
}

fn write_envelope<T: Serialize + JsonSchema>(env: &Envelope<T>, mode: OutputMode) {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    let out = match mode {
        OutputMode::Pretty => serde_json::to_string_pretty(env),
        _ => serde_json::to_string(env),
    };
    match out {
        Ok(s) => {
            let _ = writeln!(handle, "{s}");
        }
        Err(e) => {
            let _ = writeln!(
                handle,
                r#"{{"success":false,"error":{{"code":"INTERNAL_ERROR","message":"failed to serialize envelope: {e}"}}}}"#
            );
        }
    }
}
