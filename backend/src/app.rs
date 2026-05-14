use crate::{auth::AuthService, config::Config, embedding::EmbeddingClient, error::AppError, routes, static_files};
use axum::{
    extract::{ConnectInfo, Request, State},
    http::HeaderMap,
    middleware::{self, Next},
    response::Response,
    routing::get_service,
    Router,
};
use sqlx::PgPool;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, RwLock};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pool: PgPool,
    pub auth: AuthService,
    pub embeddings: EmbeddingClient,
    pub kanban_cache: KanbanCache,
    pub rate_limiter: RateLimiter,
}

#[derive(Clone, Default)]
pub struct KanbanCache {
    inner: Arc<RwLock<Option<KanbanCacheEntry>>>,
}

#[derive(Clone)]
struct KanbanCacheEntry {
    cached_at: Instant,
    response: routes::KanbanResponse,
}

impl KanbanCache {
    pub async fn get(&self, ttl: Duration) -> Option<routes::KanbanResponse> {
        let cache = self.inner.read().await;
        let entry = cache.as_ref()?;
        if entry.cached_at.elapsed() > ttl {
            return None;
        }

        Some(entry.response.clone())
    }

    pub async fn set(&self, response: routes::KanbanResponse) {
        let mut cache = self.inner.write().await;
        *cache = Some(KanbanCacheEntry {
            cached_at: Instant::now(),
            response,
        });
    }

    pub async fn invalidate(&self) {
        let mut cache = self.inner.write().await;
        *cache = None;
    }
}

#[derive(Clone, Default)]
pub struct RateLimiter {
    inner: Arc<Mutex<HashMap<IpAddr, RateLimitEntry>>>,
}

#[derive(Clone, Copy)]
struct RateLimitEntry {
    window_started_at: Instant,
    requests: u32,
}

impl RateLimiter {
    pub async fn check(&self, ip: IpAddr, max_requests: u32, window: Duration) -> bool {
        let mut entries = self.inner.lock().await;
        let now = Instant::now();
        entries.retain(|_, entry| now.duration_since(entry.window_started_at) <= window.saturating_mul(2));

        let entry = entries.entry(ip).or_insert(RateLimitEntry {
            window_started_at: now,
            requests: 0,
        });

        if now.duration_since(entry.window_started_at) > window {
            entry.window_started_at = now;
            entry.requests = 0;
        }

        entry.requests += 1;
        entry.requests <= max_requests
    }
}

pub async fn build_router(config: Arc<Config>, pool: PgPool) -> Router {
    let auth = AuthService::new(config.clone());
    let embeddings = EmbeddingClient::new(config.clone());
    let state = AppState {
        config: config.clone(),
        pool,
        auth,
        embeddings,
        kanban_cache: KanbanCache::default(),
        rate_limiter: RateLimiter::default(),
    };
    let shared_state = Arc::new(state);

    Router::new()
        .nest(
            "/api",
            routes::api_router()
                .route_layer(middleware::from_fn_with_state(shared_state.clone(), rate_limit_middleware)),
        )
        .route_service("/", get_service(static_files::index_file(&config.static_dir)))
        .route_service("/kanban", get_service(static_files::index_file(&config.static_dir)))
        .nest_service("/assets", static_files::assets_service(&config.static_dir))
        .layer(TraceLayer::new_for_http())
        .with_state(shared_state)
}

async fn rate_limit_middleware(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let ip = client_ip(request.headers(), request.extensions())
        .unwrap_or(IpAddr::from([0, 0, 0, 0]));
    let allowed = state
        .rate_limiter
        .check(
            ip,
            state.config.ip_rate_limit_requests,
            Duration::from_secs(state.config.ip_rate_limit_window_seconds),
        )
        .await;

    if !allowed {
        return Err(AppError::TooManyRequests(
            "ip rate limit exceeded, please slow down".to_string(),
        ));
    }

    Ok(next.run(request).await)
}

fn client_ip(headers: &HeaderMap, extensions: &http::Extensions) -> Option<IpAddr> {
    if let Some(forwarded_for) = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .and_then(|value| value.parse::<IpAddr>().ok())
    {
        return Some(forwarded_for);
    }

    if let Some(real_ip) = headers
        .get("x-real-ip")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<IpAddr>().ok())
    {
        return Some(real_ip);
    }

    extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|connect_info| connect_info.0.ip())
}
