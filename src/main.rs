use std::sync::Arc;

use axum::{
    Router,
    extract::{DefaultBodyLimit, State},
    response::IntoResponse,
    routing::{get, post},
};
use state::{SharedState, humanize_bytes};
use tokio::net::TcpListener;
use tokio_cron_scheduler::{Job, JobScheduler};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
// mod middleware;
mod notifier;
mod purge;
mod routes;
mod state;
mod templating;
mod track;

const ASSET_FAVICON_ICO: &[u8] = include_bytes!("../assets/favicon.ico");
const ASSET_FAVICON_PNG: &[u8] = include_bytes!("../assets/favicon.png");

#[tokio::main]
async fn main() {
    // load the configuration file
    let config = config::IhaCdnConfig::load();

    let merged_env_trace = "ihacdn=debug,tower_http=debug,axum::rejection=trace";

    // Initialize tracing logger
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .map(|filter| {
                    let split_filter = merged_env_trace.split(',').collect::<Vec<&str>>();
                    let directives = split_filter
                        .iter()
                        .fold(filter, |acc, &x| acc.add_directive(x.parse().unwrap()));
                    directives
                })
                .unwrap_or_else(|_| merged_env_trace.parse().unwrap()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let version = env!("CARGO_PKG_VERSION");
    tracing::info!("💭 Starting ihaCDN v{}", version);

    if !config.verify() {
        tracing::error!("🔌💥 Configuration file is invalid");
        std::process::exit(1);
    }

    tracing::info!("🔌 Loading services...");
    tracing::info!("🔌📒 Loading Redis database...");
    let redis_handle = match redis::Client::open(config.redis.clone()) {
        Ok(client) => {
            tracing::info!("🔌⚡ Connected to Redis");
            Arc::new(client)
        }
        Err(e) => {
            tracing::error!("🔌💥 Failed to connect to Redis: {}", e);
            std::process::exit(1);
        }
    };

    let state = state::SharedState {
        config: Arc::new(config.clone()),
        redis: redis_handle,
    };
    let shared_state = Arc::new(state);

    tracing::info!("🚀 Starting server...");
    let app = Router::new()
        .route("/", get(index))
        .route("/{id_path}", get(routes::reader::file_reader))
        .route("/{id_path}/raw", get(routes::reader::file_reader_raw))
        .route("/_/health", get(|| async { "OK" }))
        .route(
            "/upload",
            // Disable limiting the body size
            post(routes::uploads::uploads_file).layer(DefaultBodyLimit::disable()),
        )
        .route("/short", post(routes::uploads::shorten_url))
        .route("/favicon.ico", get(index_favicons_ico))
        .route("/static/img/favicon.ico", get(index_favicons_ico))
        .route("/static/img/favicon.png", get(index_favicons_png))
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(vec![
                    // GET/POST for GraphQL stuff
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    // HEAD for additional metadata
                    axum::http::Method::HEAD,
                    // OPTIONS for CORS preflight
                    axum::http::Method::OPTIONS,
                    // CONNECT for other stuff
                    axum::http::Method::CONNECT,
                ])
                .allow_headers(tower_http::cors::Any),
        )
        .with_state(Arc::clone(&shared_state));

    tracing::info!("🌐 Creating HTTP listener...");
    let listener = TcpListener::bind(format!("{}:{}", config.host.clone(), config.port))
        .await
        .unwrap();

    // Start tasks
    tracing::info!("⚡ Preparing task scheduler...");
    let mut scheduler = JobScheduler::new().await.unwrap();
    let cloned_state = Arc::clone(&shared_state);
    let job_purge = Job::new_cron_job_async("0 0 0 * * *", move |_uuid, _lock| {
        Box::pin({
            let state_val = cloned_state.clone();
            async move {
                match purge::purge_task(state_val).await {
                    Ok(_) => (),
                    Err(e) => {
                        tracing::error!("Purge task failed: {}", e);
                    }
                }
            }
        })
    })
    .unwrap();

    let job_purge_uuid = scheduler.add(job_purge).await.unwrap();
    tracing::info!("⚡ Starting task scheduler...");
    scheduler.start().await.unwrap();

    // Spawn the axum server
    let local_addr = listener.local_addr().unwrap();
    tracing::info!("🌍 Fast serving at http://{}", local_addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();

    // Stop tasks
    tracing::info!("🔕 Shutting down task scheduler...");
    scheduler.remove(&job_purge_uuid).await.unwrap();
    scheduler.shutdown().await.unwrap();
    tracing::info!("🔕 Shutting down server...");
}

async fn index(State(state): State<Arc<SharedState>>) -> impl IntoResponse {
    let retention = if state.config.retention.enable {
        Some(templating::TemplateIndexRetention {
            min_age: state.config.retention.min_age.to_string(),
            max_age: state.config.retention.max_age.to_string(),
        })
    } else {
        None
    };

    let template = templating::TemplateIndex {
        https_mode: state.config.https_mode,
        hostname: state.config.hostname.clone(),
        filesize_limit: state
            .config
            .storage
            .filesize_limit
            .map(|v| humanize_bytes(v * 1024)),
        blacklist_extensions: state.config.blocklist.extensions.clone(),
        blacklist_ctypes: state.config.blocklist.content_types.clone(),
        file_retention: retention,
    };

    templating::HtmlTemplate::new(template)
}

async fn index_favicons_ico() -> impl IntoResponse {
    let etag = format!("ihacdn-favicons-ico-{}", env!("CARGO_PKG_VERSION"));

    axum::http::Response::builder()
        .status(axum::http::StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, "image/x-icon")
        .header(
            axum::http::header::CACHE_CONTROL,
            "public, max-age=604800, immutable",
        )
        .header(axum::http::header::ETAG, etag)
        .body(axum::body::Body::from(ASSET_FAVICON_ICO))
        .unwrap()
}

async fn index_favicons_png() -> impl IntoResponse {
    let etag = format!("ihacdn-favicons-png-{}", env!("CARGO_PKG_VERSION"));

    axum::http::Response::builder()
        .status(axum::http::StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, "image/png")
        .header(
            axum::http::header::CACHE_CONTROL,
            "public, max-age=604800, immutable",
        )
        .header(axum::http::header::ETAG, etag)
        .body(axum::body::Body::from(ASSET_FAVICON_PNG))
        .unwrap()
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("🔕 Received Ctrl-C, shutting down...");
        }
        _ = terminate => {
            tracing::info!("🔕 Received SIGTERM, shutting down...");
        }
    }
}
