//! ドメイン操作 (auth / assignments / slides / drive) を 1 batch にまとめる
//! 高レベル抽象。

pub mod assignment_write;
pub mod auth_google;
pub mod auth_moocs;
pub mod drive;
pub mod navigation;
pub mod slides;
