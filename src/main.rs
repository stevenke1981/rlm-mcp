use rlm_mcp::{cli, McpServer};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        if let Err(e) = cli::run_cli(&args) {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        return;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("rlm_mcp=info".parse().unwrap()),
        )
        .with_writer(std::io::stderr)
        .init();

    if let Err(e) = McpServer::new().serve_stdio().await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
