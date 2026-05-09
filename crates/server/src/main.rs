use anyhow::Result;
use tokio::net::TcpListener;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let addr =
        std::env::var("ICARUST_LISTEN").unwrap_or_else(|_| protocol::DEFAULT_ADDR.to_string());
    let listener = TcpListener::bind(&addr).await?;
    info!(%addr, "icarust server listening");
    server::run_with_listener(listener).await
}
