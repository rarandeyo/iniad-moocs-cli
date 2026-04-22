use std::io::{self, Write};

use clap::ValueEnum;
use imoocs_core::envelope::{Envelope, ErrorDetail};
use schemars::JsonSchema;
use serde::Serialize;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Debug, Default, Clone, Copy, ValueEnum, PartialEq, Eq)]
#[value(rename_all = "lowercase")]
pub enum OutputMode {
    /// Default. Human-facing verbs (`doctor`, `auth *`, `setup`) render a
    /// human-readable summary. Agent-facing verbs emit a pretty JSON envelope
    /// regardless of this flag since they carry structured data.
    #[default]
    Text,
    /// Emit a pretty JSON envelope. Use this in agents / CI to force
    /// machine-readable output from human-facing verbs too.
    Json,
}

/// Tracing は 2 モード:
///
/// * `--format json` (agent / CI) — info level、JSON formatter。stderr に
///   構造化ログが流れるので agent からも parse できる。
/// * `--format text` (default, 人間向け) — warn level、`WARN` / `ERROR` だけ
///   を compact text で stderr に出す。成功時の info は抑制し、setup /
///   auth login が出す `eprintln!` の進捗行だけが目に入るようにする。
///
/// `--debug` は両モードで debug level に格上げ、`--quiet` は error only。
pub fn init_tracing(debug: bool, quiet: bool, format: OutputMode) {
    let json_mode = matches!(format, OutputMode::Json);
    let level = if debug {
        "imoocs=debug,imoocs_core=debug"
    } else if quiet {
        "imoocs=error,imoocs_core=error"
    } else if json_mode {
        "imoocs=info,imoocs_core=info"
    } else {
        "imoocs=warn,imoocs_core=warn"
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    let registry = tracing_subscriber::registry().with(filter);
    let _ = if json_mode {
        registry
            .with(fmt::layer().json().with_writer(io::stderr).with_ansi(false))
            .try_init()
    } else {
        registry
            .with(
                fmt::layer()
                    .without_time()
                    .with_target(false)
                    .compact()
                    .with_writer(io::stderr),
            )
            .try_init()
    };
}

/// Agent-facing verbs は format 指定に関わらず常に pretty JSON envelope を出す。
/// `_mode` は将来拡張 (および呼び出し側の API 維持) のためだけに受け取る。
pub fn emit_success<T: Serialize + JsonSchema>(data: T, _mode: OutputMode) {
    let env: Envelope<T> = Envelope::success(data);
    write_envelope(&env);
}

/// Human-facing verbs 用。`OutputMode::Text` なら `render(&data)` の文字列を
/// stdout に出し、`OutputMode::Json` なら pretty JSON envelope を出す。
/// `render` は末尾改行不要 (writeln! が 1 個足す)。
pub fn emit_success_text<T: Serialize + JsonSchema>(data: T, mode: OutputMode, render: impl FnOnce(&T) -> String) {
    match mode {
        OutputMode::Text => {
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            let text = render(&data);
            let _ = writeln!(handle, "{text}");
        }
        OutputMode::Json => {
            let env: Envelope<T> = Envelope::success(data);
            write_envelope(&env);
        }
    }
}

pub fn emit_failure<T: Serialize + JsonSchema>(err: &ErrorDetail) {
    let env: Envelope<T> = Envelope::failure(err.clone());
    write_envelope(&env);
}

fn write_envelope<T: Serialize + JsonSchema>(env: &Envelope<T>) {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    match serde_json::to_string_pretty(env) {
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
