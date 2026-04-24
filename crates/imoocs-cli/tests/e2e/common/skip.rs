/// `$key` の env が空でなければ値を返し、無ければ `[skip]` ログを出して
/// `return` する macro。
///
/// 使い方: `let user = require_env!("IMOOCS_E2E_USERNAME");`
/// `cargo test -- --nocapture` で skip 状況が見える。
#[macro_export]
macro_rules! require_env {
    ($key:literal) => {{
        match std::env::var($key) {
            Ok(v) if !v.is_empty() => v,
            _ => {
                eprintln!("[skip] {}: env {} not set", module_path!(), $key);
                return;
            }
        }
    }};
}
