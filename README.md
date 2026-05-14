# Think Alike

Think Alike is a full-stack app for publishing thoughts, embedding them with OpenAI, and exploring nearby ideas through graph and cluster views.

## Stack

- Frontend: Vue 3 + TypeScript + Element Plus + Vite
- Backend: Rust + axum + sqlx
- Database: PostgreSQL + pgvector
- Auth: GitHub OAuth

## Features

- GitHub login with allow-list and block-list controls
- Thought publishing with OpenAI embedding generation
- Daily publish rate limit, defaulting to 30 thoughts per user
- Similar-thought graph centered on a chosen thought
- Kanban-style floating cluster map with recent-thought bias
- Single-service deployment where the Rust app serves the built frontend

## Environment

Copy `.env.example` to `.env` and fill in the required values.

Use `localhost:55432` in `.env` for host-based development. `docker compose` overrides `DATABASE_URL` for the app container so it can reach the database at the Docker service name `db`.

Important variables:

- `DATABASE_URL`
- `APP_URL`
- `SESSION_SECRET`
- `GITHUB_CLIENT_ID`
- `GITHUB_CLIENT_SECRET`
- `OPENAI_API_KEY`

Optional variables:

- `GITHUB_ALLOWED_USERS`: comma-separated GitHub logins
- `GITHUB_BLOCKED_USERS`: comma-separated GitHub logins
- `OPENAI_EMBEDDING_URL`: defaults to `https://api.openai.com/v1/embeddings`
- `OPENAI_EMBEDDING_MODEL`: defaults to `text-embedding-3-small`
- `THOUGHTS_PER_DAY`: defaults to `30`

`SESSION_SECRET` must be at least 32 characters.

## Local development

1. Start PostgreSQL with pgvector:

```bash
docker compose up db -d
```

2. Install frontend dependencies:

```bash
npx pnpm install
```

3. Run the frontend dev server:

```bash
npx pnpm --dir frontend dev
```

4. Run the backend:

```bash
cargo run --manifest-path backend/Cargo.toml
```

The backend listens on `http://localhost:3000` and the Vite dev server runs on `http://localhost:5173`.
The Docker PostgreSQL instance is published on `localhost:55432` to avoid colliding with any host PostgreSQL server already using `5432`.

## Production build

Frontend build:

```bash
npx pnpm --dir frontend build
```

Backend check:

```bash
cargo check --manifest-path backend/Cargo.toml
```

Container build:

```bash
docker build -t think-alike .
```

## Docker compose

To run the full stack:

```bash
docker compose up --build
```

The runtime container uses a non-root user and only includes the compiled backend binary, frontend assets, and runtime certificates.

### Qwen + llama.cpp compose

To run the app against a local `llama.cpp` embedding server with `ai/qwen3-embedding:4B-Q4_K_M`, place the GGUF model at:

```bash
./models/qwen3-embedding_4b.gguf
```

Then start the alternate stack:

```bash
docker compose -f docker-compose-qwen-v100.yaml up --build
```

This compose file adds an `embeddings` service and overrides the app env to use:

- `OPENAI_API_KEY=dummy`
- `OPENAI_EMBEDDING_URL=http://embeddings:8080/v1/embeddings`
- `OPENAI_EMBEDDING_MODEL=ai/qwen3-embedding

It is configured for an NVIDIA GPU-backed `llama.cpp` container and publishes the embedding server on `localhost:8080`.
