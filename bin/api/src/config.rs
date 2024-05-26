use dotenvy;
use eyre::Context;
use lazy_static::lazy_static;
use std::{env, ffi::OsStr, sync::Once};

static DOTENV_INIT: Once = Once::new();

fn get_env_var<K: AsRef<OsStr>>(k: K) -> Result<String, env::VarError> {
    if cfg!(test) || cfg!(feature = "local") {
        DOTENV_INIT.call_once(|| {
            let manifest_dir =
                std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR env var not set");

            // Load the .env relative to the crate root
            dotenvy::from_path(format!("{manifest_dir}/.env")).expect(".env not found");
        });
    }

    env::var(k)
}

lazy_static! {
    pub static ref DB_PATH: String = get_env_var("DB_PATH")
        .wrap_err("Failed to read DB_PATH from env")
        .unwrap();
    pub static ref PORT: String = get_env_var("PORT")
        .wrap_err("Failed to read PORT from env")
        .unwrap();
    pub static ref RUST_LOG: String =
        get_env_var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
}
