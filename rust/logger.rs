use std::env;
use tracing_subscriber::EnvFilter;

pub fn init_logging() {
    let level = env::var("LOG_LEVEL").unwrap_or_else(|_| "INFO".to_string());
    let level = level.to_lowercase();

    let filter = match env::var("RUST_LOG") {
        Ok(rust_log) => EnvFilter::new(rust_log),
        Err(_) => EnvFilter::new(level),
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
