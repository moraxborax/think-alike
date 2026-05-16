use crate::{
    app::AppState,
    auth::{authenticated_user_from_headers, AuthenticatedUser, GithubUser},
    clustering::project_to_2d,
    error::AppError,
};
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{Duration, Utc};
use pgvector::Vector;
use rand::Rng;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Row};
use std::{cmp::Ordering, collections::HashMap, sync::Arc, time::Duration as StdDuration};
use uuid::Uuid;

pub fn api_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(health))
        .route("/auth/github/start", get(start_github_auth))
        .route("/auth/github/callback", get(finish_github_auth))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(current_user))
        .route("/thoughts", get(list_my_thoughts).post(create_thought))
        .route("/thoughts/:thought_id", delete(delete_thought))
        .route("/thoughts/:thought_id/refresh", post(refresh_thought))
        .route("/thoughts/:thought_id/similar", get(similar_thoughts))
        .route("/kanban", get(kanban_graph))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}

#[derive(Deserialize)]
struct GithubCallback {
    code: String,
    state: String,
}

async fn start_github_auth(State(state): State<Arc<AppState>>) -> Result<Response, AppError> {
    let oauth_state = state.auth.new_oauth_state().await;
    let location = format!(
        "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&scope=read:user&state={}",
        urlencoding::encode(&state.config.github_client_id),
        urlencoding::encode(&state.config.github_redirect_uri),
        urlencoding::encode(&oauth_state)
    );

    let mut response = Redirect::temporary(&location).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&state.auth.build_state_cookie(&oauth_state))
            .map_err(|_| AppError::Internal(anyhow::anyhow!("invalid oauth cookie")))?,
    );
    Ok(response)
}

#[derive(Deserialize)]
struct GithubAccessToken {
    access_token: String,
}

async fn finish_github_auth(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<GithubCallback>,
) -> Result<Response, AppError> {
    state.auth.verify_state_cookie(&headers, &query.state)?;
    state.auth.verify_oauth_state(&query.state).await?;

    let client = Client::new();
    let token_response = client
        .post("https://github.com/login/oauth/access_token")
        .header(header::ACCEPT, "application/json")
        .form(&serde_json::json!({
            "client_id": state.config.github_client_id,
            "client_secret": state.config.github_client_secret,
            "code": query.code,
            "redirect_uri": state.config.github_redirect_uri,
            "state": query.state,
        }))
        .send()
        .await?;

    if !token_response.status().is_success() {
        return Err(AppError::Upstream("failed to exchange GitHub code".to_string()));
    }

    let token: GithubAccessToken = token_response.json().await?;
    let user_response = client
        .get("https://api.github.com/user")
        .header(header::USER_AGENT, "think-alike")
        .bearer_auth(token.access_token)
        .send()
        .await?;

    if !user_response.status().is_success() {
        return Err(AppError::Upstream("failed to fetch GitHub user".to_string()));
    }

    let github_user: GithubUser = user_response.json().await?;
    state.auth.is_user_allowed(&github_user.login)?;

    upsert_user(&state, &github_user).await?;
    let session = state.auth.encode_session(&github_user)?;
    let secure = state.config.app_url.starts_with("https://");

    let mut response = Redirect::to("/").into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&state.auth.build_session_cookie(&session, secure))
            .map_err(|_| AppError::Internal(anyhow::anyhow!("invalid session cookie")))?,
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_static("oauth_state=; HttpOnly; Path=/; Max-Age=0; SameSite=Lax"),
    );
    Ok(response)
}

async fn logout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let _user = require_user(&state, &headers)?;
    let secure = state.config.app_url.starts_with("https://");
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&state.auth.build_clear_session_cookie(secure))
            .map_err(|_| AppError::Internal(anyhow::anyhow!("invalid clear session cookie")))?,
    );
    Ok(response)
}

#[derive(Serialize)]
struct CurrentUserResponse {
    github_id: i64,
    login: String,
    avatar_url: Option<String>,
}

async fn current_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<CurrentUserResponse>, AppError> {
    let user = require_user(&state, &headers)?;
    upsert_authenticated_user(&state, &user).await?;
    Ok(Json(CurrentUserResponse {
        github_id: user.github_id,
        login: user.login,
        avatar_url: user.avatar_url,
    }))
}

#[derive(Deserialize)]
struct CreateThoughtRequest {
    title: String,
    description: String,
}

#[derive(Serialize)]
struct ThoughtResponse {
    id: Uuid,
    title: String,
    description: String,
    created_at: chrono::DateTime<Utc>,
    age_hours: i64,
    author_github_id: i64,
    author_login: String,
    author_avatar_url: Option<String>,
}

#[derive(Clone, FromRow)]
struct ThoughtRow {
    id: Uuid,
    user_id: i64,
    title: String,
    description: String,
    created_at: chrono::DateTime<Utc>,
    embedding: Vector,
    embedding_dimensions: i32,
    author_login: String,
    author_avatar_url: Option<String>,
}

async fn list_my_thoughts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<ThoughtResponse>>, AppError> {
    let user = require_user(&state, &headers)?;
    upsert_authenticated_user(&state, &user).await?;
    let thoughts = sqlx::query_as::<_, ThoughtRow>(
        "select thoughts.id, thoughts.user_id, thoughts.title, thoughts.description, thoughts.created_at, thoughts.embedding, thoughts.embedding_dimensions, users.login as author_login, users.avatar_url as author_avatar_url from thoughts join users on users.github_id = thoughts.user_id where thoughts.user_id = $1 order by thoughts.created_at desc limit 100",
    )
    .bind(user.github_id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(
        thoughts
            .into_iter()
            .map(|thought| ThoughtResponse {
                id: thought.id,
                title: thought.title,
                description: thought.description,
                created_at: thought.created_at,
                age_hours: age_hours_since(thought.created_at),
                author_github_id: thought.user_id,
                author_login: thought.author_login,
                author_avatar_url: thought.author_avatar_url,
            })
            .collect(),
    ))
}

async fn create_thought(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateThoughtRequest>,
) -> Result<(StatusCode, Json<ThoughtResponse>), AppError> {
    let user = require_user(&state, &headers)?;
    upsert_authenticated_user(&state, &user).await?;
    let title = payload.title.trim();
    let description = payload.description.trim();
    if title.is_empty() || description.is_empty() {
        return Err(AppError::BadRequest(
            "title and description are required".to_string(),
        ));
    }
    if title.len() > 120 || description.len() > 5000 {
        return Err(AppError::BadRequest(
            "title or description is too long".to_string(),
        ));
    }

    let since = Utc::now() - Duration::days(1);
    let published_today: i64 = sqlx::query_scalar(
        "select count(*) from thoughts where user_id = $1 and created_at >= $2",
    )
    .bind(user.github_id)
    .bind(since)
    .fetch_one(&state.pool)
    .await?;
    if published_today >= state.config.thoughts_per_day {
        return Err(AppError::TooManyRequests(format!(
            "daily limit reached: {} thoughts per day",
            state.config.thoughts_per_day
        )));
    }

    let embedding = state.embeddings.embed_text(title, description).await?;
    let embedding_dimensions = embedding.len() as i32;
    let thought_id = Uuid::new_v4();
    let created_at = Utc::now();

    sqlx::query(
        "insert into thoughts (id, user_id, title, description, embedding, embedding_dimensions, created_at) values ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(thought_id)
    .bind(user.github_id)
    .bind(title)
    .bind(description)
    .bind(Vector::from(embedding))
    .bind(embedding_dimensions)
    .bind(created_at)
    .execute(&state.pool)
    .await?;

    invalidate_kanban_cache(&state).await;

    Ok((
        StatusCode::CREATED,
        Json(ThoughtResponse {
            id: thought_id,
            title: title.to_string(),
            description: description.to_string(),
            created_at,
            age_hours: 0,
            author_github_id: user.github_id,
            author_login: user.login.clone(),
            author_avatar_url: user.avatar_url.clone(),
        }),
    ))
}

async fn delete_thought(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(thought_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let user = require_user(&state, &headers)?;
    upsert_authenticated_user(&state, &user).await?;
    let result = sqlx::query("delete from thoughts where id = $1 and user_id = $2")
        .bind(thought_id)
        .bind(user.github_id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("thought not found".to_string()));
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn refresh_thought(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(thought_id): Path<Uuid>,
) -> Result<Json<ThoughtResponse>, AppError> {
    let user = require_user(&state, &headers)?;
    upsert_authenticated_user(&state, &user).await?;
    let updated = sqlx::query_as::<_, ThoughtRow>(
        "update thoughts set created_at = now() where id = $1 and user_id = $2 returning id, user_id, title, description, created_at, embedding, embedding_dimensions, $3 as author_login, $4 as author_avatar_url",
    )
    .bind(thought_id)
    .bind(user.github_id)
    .bind(&user.login)
    .bind(&user.avatar_url)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("thought not found".to_string()))?;

    Ok(Json(ThoughtResponse {
        id: updated.id,
        title: updated.title,
        description: updated.description,
        created_at: updated.created_at,
        age_hours: age_hours_since(updated.created_at),
        author_github_id: updated.user_id,
        author_login: updated.author_login,
        author_avatar_url: updated.author_avatar_url,
    }))
}

#[derive(Serialize)]
struct SimilarThoughtNode {
    id: Uuid,
    title: String,
    description: String,
    age_hours: i64,
    author_github_id: i64,
    author_login: String,
    author_avatar_url: Option<String>,
    score: f32,
    x: f32,
    y: f32,
    center: bool,
}

#[derive(Serialize)]
struct SimilarThoughtGraph {
    center_id: Uuid,
    nodes: Vec<SimilarThoughtNode>,
}

async fn similar_thoughts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(thought_id): Path<Uuid>,
) -> Result<Json<SimilarThoughtGraph>, AppError> {
    let user = require_user(&state, &headers)?;
    upsert_authenticated_user(&state, &user).await?;
    let center = sqlx::query_as::<_, ThoughtRow>(
        "select thoughts.id, thoughts.user_id, thoughts.title, thoughts.description, thoughts.created_at, thoughts.embedding, thoughts.embedding_dimensions, users.login as author_login, users.avatar_url as author_avatar_url from thoughts join users on users.github_id = thoughts.user_id where thoughts.id = $1 and thoughts.user_id = $2",
    )
    .bind(thought_id)
    .bind(user.github_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("thought not found".to_string()))?;

    let related = sqlx::query(
        "select thoughts.id, thoughts.user_id, thoughts.title, thoughts.description, thoughts.created_at, users.login as author_login, users.avatar_url as author_avatar_url, 1 - (thoughts.embedding <=> $1) as score from thoughts join users on users.github_id = thoughts.user_id where thoughts.id != $2 and thoughts.embedding_dimensions = $3 order by thoughts.embedding <=> $1 limit 18",
    )
    .bind(center.embedding.clone())
    .bind(center.id)
    .bind(center.embedding_dimensions)
    .fetch_all(&state.pool)
    .await?;

    let mut nodes = vec![SimilarThoughtNode {
        id: center.id,
        title: center.title.clone(),
        description: center.description.clone(),
        age_hours: age_hours_since(center.created_at),
        author_github_id: center.user_id,
        author_login: center.author_login.clone(),
        author_avatar_url: center.author_avatar_url.clone(),
        score: 1.0,
        x: 0.0,
        y: 0.0,
        center: true,
    }];

    let visible_related = related
        .into_iter()
        .map(|row| {
            Ok((
                row.try_get::<Uuid, _>("id")?,
                row.try_get::<String, _>("title")?,
                row.try_get::<String, _>("description")?,
                row.try_get::<chrono::DateTime<Utc>, _>("created_at")?,
                row.try_get::<i64, _>("user_id")?,
                row.try_get::<String, _>("author_login")?,
                row.try_get::<Option<String>, _>("author_avatar_url")?,
                row.try_get::<Option<f64>, _>("score")?.unwrap_or(0.0) as f32,
            ))
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;

    let best_score = visible_related.first().map(|row| row.7).unwrap_or(0.0);
    let minimum_score = (best_score - 0.14).max(0.58);

    for (index, row) in visible_related
        .into_iter()
        .filter(|row| row.7 >= minimum_score)
        .take(10)
        .enumerate()
    {
        let score = row.7;
        let angle = (index as f32 / 10.0) * std::f32::consts::TAU;
        let distance = (1.15 - score.clamp(0.0, 1.0)).max(0.15) * 220.0;
        nodes.push(SimilarThoughtNode {
            id: row.0,
            title: row.1,
            description: row.2,
            age_hours: age_hours_since(row.3),
            author_github_id: row.4,
            author_login: row.5,
            author_avatar_url: row.6,
            score,
            x: angle.cos() * distance,
            y: angle.sin() * distance,
            center: false,
        });
    }

    Ok(Json(SimilarThoughtGraph {
        center_id: center.id,
        nodes,
    }))
}

#[derive(Clone, Serialize)]
pub struct KanbanNode {
    id: Uuid,
    title: String,
    description: String,
    author_github_id: i64,
    author_login: String,
    author_avatar_url: Option<String>,
    x: f32,
    y: f32,
    age_hours: i64,
}

#[derive(Clone, Serialize)]
pub struct KanbanResponse {
    nodes: Vec<KanbanNode>,
    normalized_stress: f32,
}

async fn kanban_graph(
    State(state): State<Arc<AppState>>,
) -> Result<Json<KanbanResponse>, AppError> {
    let thoughts = sqlx::query_as::<_, ThoughtRow>(
        "select thoughts.id, thoughts.user_id, thoughts.title, thoughts.description, thoughts.created_at, thoughts.embedding, thoughts.embedding_dimensions, users.login as author_login, users.avatar_url as author_avatar_url from thoughts join users on users.github_id = thoughts.user_id order by thoughts.created_at desc limit 400",
    )
    .fetch_all(&state.pool)
    .await?;

    let Some(dominant_dimensions) = dominant_embedding_dimensions(&thoughts) else {
        return Ok(Json(KanbanResponse {
            nodes: Vec::new(),
            normalized_stress: 0.0,
        }));
    };

    let thoughts = thoughts
        .into_iter()
        .filter(|thought| thought.embedding_dimensions == dominant_dimensions)
        .collect::<Vec<_>>();

    let thought_count = thoughts.len();
    let layout = if let Some(cached) = state
        .kanban_cache
        .get(
            StdDuration::from_secs(state.config.kanban_recompute_interval_seconds),
            thought_count,
        )
        .await
    {
        cached
    } else {
        let sampled = weighted_recent_sample(thoughts.clone(), 120);
        let embeddings = sampled
            .iter()
            .map(|thought| thought.embedding.to_vec())
            .collect::<Vec<_>>();
        let projection = project_to_2d(&embeddings);
        let layout = crate::app::KanbanLayoutEntry {
            thought_ids: sampled.iter().map(|thought| thought.id).collect(),
            positions: projection.positions,
            normalized_stress: projection.normalized_stress,
        };
        state.kanban_cache.set(thought_count, layout.clone()).await;
        layout
    };

    let thoughts_by_id = thoughts
        .into_iter()
        .map(|thought| (thought.id, thought))
        .collect::<HashMap<_, _>>();

    let mut projected = layout
        .thought_ids
        .into_iter()
        .zip(layout.positions.into_iter())
        .filter_map(|(thought_id, (x, y))| {
            let thought = thoughts_by_id.get(&thought_id)?;
            Some(KanbanNode {
                id: thought.id,
                title: thought.title.clone(),
                description: thought.description.clone(),
                author_github_id: thought.user_id,
                author_login: thought.author_login.clone(),
                author_avatar_url: thought.author_avatar_url.clone(),
                x,
                y,
                age_hours: age_hours_since(thought.created_at),
            })
        })
        .collect::<Vec<_>>();

    normalize_positions(&mut projected);
    Ok(Json(KanbanResponse {
        nodes: projected,
        normalized_stress: layout.normalized_stress,
    }))
}

fn weighted_recent_sample(mut thoughts: Vec<ThoughtRow>, target: usize) -> Vec<ThoughtRow> {
    if thoughts.len() <= target {
        return thoughts;
    }

    let now = Utc::now();
    let mut rng = rand::thread_rng();
    thoughts.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    thoughts.sort_by(|left, right| {
        let left_weight = sample_weight(now, left.created_at) * rng.gen_range(0.6f64..1.4f64);
        let right_weight = sample_weight(now, right.created_at) * rng.gen_range(0.6f64..1.4f64);
        right_weight
            .partial_cmp(&left_weight)
            .unwrap_or(Ordering::Equal)
    });
    thoughts.into_iter().take(target).collect()
}

fn sample_weight(now: chrono::DateTime<Utc>, created_at: chrono::DateTime<Utc>) -> f64 {
    let age_hours = (now - created_at).num_hours().max(0) as f64;
    1.0 / (age_hours + 2.0).powf(0.55)
}

fn age_hours_since(created_at: chrono::DateTime<Utc>) -> i64 {
    (Utc::now() - created_at).num_hours().max(0)
}

fn normalize_positions(nodes: &mut [KanbanNode]) {
    let max_x = nodes.iter().map(|node| node.x.abs()).fold(1.0, f32::max);
    let max_y = nodes.iter().map(|node| node.y.abs()).fold(1.0, f32::max);
    for node in nodes {
        node.x = (node.x / max_x) * 46.0;
        node.y = (node.y / max_y) * 46.0;
    }
}

fn dominant_embedding_dimensions(thoughts: &[ThoughtRow]) -> Option<i32> {
    use std::collections::HashMap;

    let mut counts = HashMap::<i32, usize>::new();
    for thought in thoughts {
        *counts.entry(thought.embedding_dimensions).or_default() += 1;
    }

    counts
        .into_iter()
        .max_by_key(|(dimensions, count)| (*count, *dimensions))
        .map(|(dimensions, _)| dimensions)
}

async fn upsert_user(state: &Arc<AppState>, user: &GithubUser) -> Result<(), AppError> {
    sqlx::query(
        "insert into users (github_id, login, avatar_url, updated_at) values ($1, $2, $3, now()) on conflict (github_id) do update set login = excluded.login, avatar_url = excluded.avatar_url, updated_at = now()",
    )
    .bind(user.id)
    .bind(&user.login)
    .bind(&user.avatar_url)
    .execute(&state.pool)
    .await?;
    Ok(())
}

async fn upsert_authenticated_user(
    state: &Arc<AppState>,
    user: &AuthenticatedUser,
) -> Result<(), AppError> {
    sqlx::query(
        "insert into users (github_id, login, avatar_url, updated_at) values ($1, $2, $3, now()) on conflict (github_id) do update set login = excluded.login, avatar_url = excluded.avatar_url, updated_at = now()",
    )
    .bind(user.github_id)
    .bind(&user.login)
    .bind(&user.avatar_url)
    .execute(&state.pool)
    .await?;
    Ok(())
}

fn require_user(state: &Arc<AppState>, headers: &HeaderMap) -> Result<AuthenticatedUser, AppError> {
    authenticated_user_from_headers(&state.auth, headers)
}

async fn invalidate_kanban_cache(state: &Arc<AppState>) {
    state.kanban_cache.invalidate().await;
}
