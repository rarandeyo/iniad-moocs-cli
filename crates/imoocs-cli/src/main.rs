use std::process::ExitCode;

use clap::Parser;

mod cli;
mod output;

#[tokio::main]
async fn main() -> ExitCode {
    let args = cli::Cli::parse();
    output::init_tracing(args.global.debug, args.global.quiet);

    match cli::run(args).await {
        Ok(code) => code,
        Err(err) => {
            // Unexpected top-level error; print failure envelope and exit 5.
            let detail = imoocs_core::envelope::ErrorDetail {
                code: "INTERNAL_ERROR".into(),
                message: err.to_string(),
                hint: None,
                details: None,
            };
            output::emit_failure::<serde_json::Value>(&detail);
            ExitCode::from(5)
        }
    }
}
