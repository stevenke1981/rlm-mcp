use rlm_mcp::{cli, McpServer};
use std::env;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if !args.is_empty() {
        if let Err(e) = cli::run_cli(&args) {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        return;
    }

    let log_format = env::var("RLM_LOG_FORMAT").unwrap_or_else(|_| "pretty".into());
    let filter = EnvFilter::from_default_env().add_directive("rlm_mcp=info".parse().unwrap());

    match log_format.as_str() {
        "json" => {
            tracing_subscriber::fmt()
                .json()
                .with_env_filter(filter)
                .with_writer(std::io::stderr)
                .init();
        }
        _ => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(std::io::stderr)
                .init();
        }
    }

    if let Err(e) = McpServer::new().serve_stdio().await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
