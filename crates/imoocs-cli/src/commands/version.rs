use std::process::ExitCode;

use anyhow::Result;
use serde_json::json;

use crate::cli::GlobalArgs;
use crate::output;

pub fn run(global: &GlobalArgs) -> Result<ExitCode> {
    let data = json!({
        "name": "imoocs",
        "version": env!("CARGO_PKG_VERSION"),
    });
    output::emit_success::<serde_json::Value>(data, global.format);
    Ok(ExitCode::from(0))
}
