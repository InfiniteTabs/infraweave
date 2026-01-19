use env_common::interface::initialize_project_id_and_region;
use internal_api::http_router;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();
    initialize_project_id_and_region().await;

    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .expect("Invalid port number");

    let app = http_router::create_router().layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(&addr).await?;

    log::info!("Starting local HTTP server on http://127.0.0.1:{}", port);
    println!("Server running at http://127.0.0.1:{}", port);
    println!("\nExample requests:");
    println!("  curl http://127.0.0.1:{}/api/v1/modules", port);
    println!("  curl http://127.0.0.1:{}/api/v1/projects", port);

    axum::serve(listener, app).await
}
