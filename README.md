# Think Alike
Think Alike is a full-stack app for publishing thoughts, embedding, and exploring nearby ideas through graph and cluster views.

![](./imgs/image.png)

## Stack

- Frontend: Vue 3 + TypeScript + Element Plus + Vite
- Backend: Rust + axum + sqlx
- Database: PostgreSQL + pgvector
- Auth: GitHub OAuth

## Features

- GitHub login with allow-list and block-list controls
- Thought publishing with embedding generation
- Daily publish rate limit, defaulting to 30 thoughts per user
- Similar-thought graph centered on a chosen thought
- Kanban-style floating cluster map with recent-thought bias
- Single-service deployment where the Rust app serves the built frontend

## Environment

Copy `.env.example` to `.env` and fill in the required values.

The default compose stacks do not publish PostgreSQL on the host. The app container connects to the database over the internal Docker network using `db:5432`.

Important variables:

- `POSTGRES_PASSWORD`
- `DATABASE_URL`
- `APP_URL`
- `GITHUB_REDIRECT_URI`
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
- `KANBAN_CACHE_TTL_SECONDS`: defaults to `15`
- `IP_RATE_LIMIT_REQUESTS`: defaults to `120`
- `IP_RATE_LIMIT_WINDOW_SECONDS`: defaults to `60`

`SESSION_SECRET` must be at least 32 characters.

`GITHUB_REDIRECT_URI` should match the callback URL configured in your GitHub OAuth app. If it is omitted, the backend falls back to `${APP_URL}/api/auth/github/callback`.

The public kanban API is cached in memory for `KANBAN_CACHE_TTL_SECONDS`, and all `/api` requests are protected by a per-IP fixed-window rate limit using `IP_RATE_LIMIT_REQUESTS` over `IP_RATE_LIMIT_WINDOW_SECONDS`.

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

4. Set a strong `POSTGRES_PASSWORD` in `.env` before starting the stack.

5. For host-based backend development, create a local-only DB port forward or override first. Example:

```bash
docker compose port db 5432
```

Or add a temporary local-only override such as:

```yaml
services:
  db:
    ports:
      - "127.0.0.1:55432:5432"
```

Then set `DATABASE_URL=postgres://postgres:<your-postgres-password>@localhost:55432/think_alike` in your local `.env`.

6. Run the backend:

```bash
cargo run --manifest-path backend/Cargo.toml
```

The backend listens on `http://localhost:3000` and the Vite dev server runs on `http://localhost:5173`.

For direct database access, prefer running commands inside the container instead of exposing Postgres:

```bash
docker compose exec db psql -U postgres -d think_alike
```

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

PostgreSQL is intentionally not exposed on the host by default. If you need host access for debugging, use a temporary loopback-only port mapping and a non-default password.

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
Its PostgreSQL service is also internal-only by default.
