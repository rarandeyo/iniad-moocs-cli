pub mod assignment;
pub mod auth;
pub mod completion;
pub mod confirm;
pub mod course;
pub mod doctor;
pub mod drive;
pub mod lesson;
pub mod open;
pub mod skill;
pub mod slide;
pub mod version;

use imoocs_core::config::Config;
use imoocs_core::paths::{resolve_slides_out_dir, Paths, DEFAULT_SLIDES_OUT_DIR};
use imoocs_core::Result;

/// Layer the effective slide-PDF destination onto `Paths`.
///
/// Precedence (high → low): CLI flag (`--out-dir`), `config.toml [slides] out_dir`,
/// then the built-in default (`DEFAULT_SLIDES_OUT_DIR` = `"tmp"`).
pub fn apply_slides_config(paths: Paths, cli_override: Option<&str>) -> Result<Paths> {
    let cfg = Config::load(&paths.config_file())?;
    let value = cli_override
        .or_else(|| cfg.slides.as_ref().and_then(|s| s.out_dir.as_deref()))
        .unwrap_or(DEFAULT_SLIDES_OUT_DIR);
    let dir = resolve_slides_out_dir(value, &paths.cache_dir)?;
    Ok(paths.with_slides_dir(dir))
}
