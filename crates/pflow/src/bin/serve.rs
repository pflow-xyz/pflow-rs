//! Static file server for chess integer reduction frontend.
//! Sets COEP/COOP headers required for SharedArrayBuffer (Stockfish WASM threading).
//!
//! Usage: cargo run --bin serve --features serve

use axum::Router;
use std::net::SocketAddr;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

#[tokio::main]
async fn main() {
    let serve_dir = ServeDir::new("www/chess");

    let app = Router::new()
        .fallback_service(serve_dir)
        .layer(SetResponseHeaderLayer::overriding(
            http::header::HeaderName::from_static("cross-origin-embedder-policy"),
            http::HeaderValue::from_static("require-corp"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            http::header::HeaderName::from_static("cross-origin-opener-policy"),
            http::HeaderValue::from_static("same-origin"),
        ));

    let addr = SocketAddr::from(([127, 0, 0, 1], 8090));
    println!("Serving www/chess/ on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
