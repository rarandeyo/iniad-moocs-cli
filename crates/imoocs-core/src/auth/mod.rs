pub mod google;
pub mod moocs;

pub use google::{is_logged_in_google, login_google};
pub use moocs::{is_logged_in_moocs, login_moocs, logout_local, Credentials};
