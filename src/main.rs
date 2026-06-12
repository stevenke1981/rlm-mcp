use codebase_memory_rlm_mcp::McpServer;
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("codebase_memory_rlm_mcp=info".parse().unwrap()),
        )
        .with_writer(std::io::stderr)
        .init();

    if let Err(e) = McpServer::new().run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}