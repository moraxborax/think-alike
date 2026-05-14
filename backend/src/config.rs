use anyhow::{bail, Context};
use std::{collections::HashSet, env, path::PathBuf};

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub app_url: String,
    pub github_redirect_uri: String,
    pub session_secret: String,
    pub github_client_id: String,
    pub github_client_secret: String,
    pub github_allowed_users: HashSet<String>,
    pub github_blocked_users: HashSet<String>,
    pub openai_api_key: String,
    pub openai_embedding_url: String,
    pub openai_embedding_model: String,
    pub thoughts_per_day: i64,
    pub static_dir: PathBuf,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let database_url = required("DATABASE_URL")?;
        let app_url = required("APP_URL")?;
        let github_redirect_uri = env::var("GITHUB_REDIRECT_URI")
            .unwrap_or_else(|_| format!("{app_url}/api/auth/github/callback"));
        let session_secret = required("SESSION_SECRET")?;
        let github_client_id = required("GITHUB_CLIENT_ID")?;
        let github_client_secret = required("GITHUB_CLIENT_SECRET")?;
        let openai_api_key = required("OPENAI_API_KEY")?;
        let openai_embedding_url = env::var("OPENAI_EMBEDDING_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1/embeddings".to_string());
        let openai_embedding_model = env::var("OPENAI_EMBEDDING_MODEL")
            .unwrap_or_else(|_| "text-embedding-3-small".to_string());
        let thoughts_per_day = env::var("THOUGHTS_PER_DAY")
            .ok()
            .map(|value| value.parse::<i64>())
            .transpose()
            .context("THOUGHTS_PER_DAY must be an integer")?
            .unwrap_or(30);

        if session_secret.len() < 32 {
            bail!("SESSION_SECRET must be at least 32 characters");
        }

        Ok(Self {
            database_url,
            app_url,
            github_redirect_uri,
            session_secret,
            github_client_id,
            github_client_secret,
            github_allowed_users: csv_set("GITHUB_ALLOWED_USERS"),
            github_blocked_users: csv_set("GITHUB_BLOCKED_USERS"),
            openai_api_key,
            openai_embedding_url,
            openai_embedding_model,
            thoughts_per_day,
            static_dir: PathBuf::from("frontend/dist"),
        })
    }
}

fn required(name: &str) -> anyhow::Result<String> {
    env::var(name).with_context(|| format!("missing required environment variable {name}"))
}

fn csv_set(name: &str) -> HashSet<String> {
    env::var(name)
        .unwrap_or_default()
        .split(',')
        .filter_map(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_lowercase())
        })
        .collect()
}
