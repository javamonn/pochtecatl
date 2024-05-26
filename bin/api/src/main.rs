mod config;
mod primitives;
mod routes;

use pochtecatl_db::connect as connect_db;
use primitives::AppState;

use axum::{
    extract::{MatchedPath, Request},
    http::{HeaderValue, Method},
    routing::get,
    Router,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use eyre::Result;
use std::str::FromStr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::from_str(&config::RUST_LOG).unwrap_or_default())
        .with(tracing_subscriber::fmt::layer())
        .init();

    let db = connect_db(&config::DB_PATH)?;
    let app = Router::new()
        .route("/backtests", get(routes::list_backtests::handler))
        .route(
            "/backtests/:backtest_id",
            get(routes::list_backtest_pairs::handler),
        )
        .route(
            "/backtests/:backtest_id/pairs/:pair_address",
            get(routes::get_backtest_pair::handler),
        )
        .layer(
            // CORS layer
            CorsLayer::new()
                .allow_origin("http://localhost:3000".parse::<HeaderValue>().unwrap())
                .allow_methods([Method::GET]),
        )
        .layer(
            // Tracing layer
            TraceLayer::new_for_http()
                // Create our own span for the request and include the matched path. The matched
                // path is useful for figuring out which handler the request was routed to.
                .make_span_with(|req: &Request| {
                    let method = req.method();
                    let uri = req.uri();

                    // axum automatically adds this extension.
                    let matched_path = req
                        .extensions()
                        .get::<MatchedPath>()
                        .map(|matched_path| matched_path.as_str());

                    tracing::debug_span!("request", %method, %uri, matched_path)
                })
                // By default `TraceLayer` will log 5xx responses but we're doing our specific
                // logging of errors so disable that
                .on_failure(()),
        )
        .with_state(AppState::new(db));

    let listener = tokio::net::TcpListener::bind(
        format!("[::]:{}", *config::PORT)
            .parse::<std::net::SocketAddr>()
            .expect("Failed to parse address"),
    )
    .await
    .unwrap();

    tracing::debug!("listening on {}", listener.local_addr().unwrap());

    axum::serve(listener, app).await.unwrap();

    tracing::debug!("done");

    Ok(())
}
