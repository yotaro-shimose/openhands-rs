use std::env;
use tracing_subscriber::EnvFilter;

/// Initializes the global logging system with colorized output and environment-based level filtering.
///
/// The `RUST_LOG` environment variable can be used to control the log level (default: info).
/// Example: `RUST_LOG=debug cargo run --example remote_test`
pub fn init_logging() {
    if env::var("RUST_LOG").is_err() {
        unsafe { env::set_var("RUST_LOG", "info") };
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(false)
        .with_line_number(false)
        .with_target(false)
        .init();
}
