# RakuYomi Cookie Sync Bot

Telegram bot + HTTP API for syncing browser cookies from Android (Kiwi Browser)
to KOReader (RakuYomi) devices. Also provides an OAuth bridge for tracking
service sign-in (AniList, MyAnimeList, Shikimori, Bangumi, MangaBaka).

Self-hosted services (Kavita, Komga, Suwayomi) are configured directly in the
KOReader plugin since they require local network access.

## Architecture

- **Bot** — grammY, handles `/link`, `/devices`, cookie ingestion via text messages
- **API** — Hono + `@hono/zod-openapi`, serves cookie data to the Rust backend
- **OAuth** — Bridge page for phone-based sign-in via QR code scanning
- **DB** — Turso (libSQL), stores device registrations and per-device cookie blobs
- **KV** — a Durable Object stores pairing codes and OAuth sessions
- **Webapp** — Telegram Mini App for visual cookie management

Runs on **Cloudflare Workers** with [Bun](https://bun.sh) as the package manager.

```
User -> Telegram -> bot -> Durable Object (pairing, OAuth sessions)
KOReader -> Rust Backend -> Worker -> Turso DB
KOReader -> Rust Backend -> Worker OAuth Bridge -> Tracking Services
```

## Directory Structure

```
main.ts                          # Local development entry (Node-style HTTP + long polling)
bun.lock                         # Bun lockfile
wrangler.toml                    # Cloudflare Workers configuration
src/
  worker.ts                      # Cloudflare Workers entry point
  env.ts                         # Bindings + ambient request environment
  config.ts                      # All env vars (getConfig + getOAuthConfigs)
  schemas.ts                     # Zod schemas + service constants
  server.ts                      # Route registration + webhook route
  kv.ts                          # Key/value API (pairing, OAuth sessions)
  kv/do.ts                       # Durable Object backing kv.ts
  oauth_kv.ts                    # OAuth session helpers
  turso.ts                       # Turso client & migrations
  store.ts                       # In-memory Map cache synced to Turso
  i18n.ts                        # Locale resolver
  locales/                       # en, vi, jp, zh-cn, zh-hk
  components/
    ResultPage.tsx               # OAuth result page component
    BridgePage.tsx               # OAuth bridge page component
  logic/
    pkce.ts                      # PKCE code verifier/challenge generation
  services/oauth/
    anilist.ts                   # AniList token exchange
    myanimelist.ts               # MAL token exchange
    shikimori.ts                 # Shikimori token exchange
    bangumi.ts                   # Bangumi token exchange
    mangabaka.ts                 # MangaBaka token exchange (PKCE S256)
  utils/
    oauth.tsx                    # Route helpers (error/success/validateSession)
    schema.ts                    # Cookie validation schemas
    cookie.ts                    # Cookie parsing utilities
    telegram-webapp.ts           # initData verification
  routes/                        # Browser-facing routes (NOT API)
    oauth/
      [service]/[sessionId].tsx  # GET /oauth/:service/:sessionId (bridge page)
      anilist/callback.ts        # AniList callback (state=sessionId)
      myanimelist/callback.ts    # MAL callback (state=sessionId)
      shikimori/callback.ts      # Shikimori callback (state=sessionId)
      bangumi/callback.ts        # Bangumi callback (state=sessionId)
      mangabaka/callback.ts      # MangaBaka callback (state=sessionId)
  api/                           # JSON API routes only
    middleware/auth.ts
    routes/
      health.ts
      webapp.tsx
      api/
        oauth/session.ts         # POST /api/oauth/session
        oauth/status.ts          # GET /api/oauth/status/:sessionId
        pairing/generate.ts
        pairing/status.ts
        cookie/get.ts, devices.ts, sync-all.ts, notify-needs-update.ts
        webapp/data.ts, cookies.ts, clear.ts, unlink.ts
```

## Local development

```sh
bun install          # install dependencies
cp .env.example .env # then fill in the values

bun run dev          # Hono API on :8788 + grammY long polling
bun run dev:worker   # wrangler dev (webhook mode, real Durable Object)

bun run typecheck    # tsc --noEmit
bun test             # unit tests (hermetic: no DB, in-memory KV)
```

`bun run dev` runs the whole stack in one process, like `deno task dev` used to.
Careful: long polling calls `deleteWebhook`, so a deployed instance stops
receiving updates until it re-registers its webhook. Run
`DEV_POLLING=false bun run dev` to serve the API only.

`bun run dev:worker` matches production - Workers cannot long-poll, so the bot
only works through the webhook there. A Worker binds `.env` as secrets as well,
so one `.env` covers both commands.

## Deployment

### Cloudflare Workers

1. Log in and deploy:

   ```sh
   bun x wrangler login
   bun run deploy
   ```

2. Set the secrets (Wrangler reads `.env` locally, but production needs them
   stored on Cloudflare):

   ```sh
   bun x wrangler secret put BOT_TOKEN
   bun x wrangler secret put TURSO_AUTH_TOKEN
   bun x wrangler secret put WEBHOOK_SECRET
   ```

   Repeat for every OAuth client secret that is configured. `PUBLIC_URL` and
   `USE_WEBHOOK` live in `wrangler.toml` under `[vars]` - edit them there if the
   URL changes.

3. The Worker registers the Telegram webhook on the first request it serves.
   Set it by hand if needed:

   ```
   https://api.telegram.org/bot<BOT_TOKEN>/setWebhook?url=https://<your-worker>.workers.dev/webhook&secret_token=<WEBHOOK_SECRET>
   ```

### Environment variables

| Variable                | Required | Description                                                  |
| ----------------------- | -------- | ------------------------------------------------------------ |
| `BOT_TOKEN`             | Yes      | Telegram bot token from [@BotFather](https://t.me/BotFather) |
| `TURSO_DB_URL`          | Yes      | Turso database URL                                           |
| `TURSO_AUTH_TOKEN`      | Yes      | Turso database auth token                                    |
| `PUBLIC_URL`            | Yes      | Public URL of the deployed server (OAuth + webhook)          |
| `WEBHOOK_SECRET`        | No       | Secret checked on `/webhook`                                 |
| `USE_WEBHOOK`           | No       | Set to `true` for webhook mode (the only one Workers allows)  |
| `ANILIST_CLIENT_ID`     | No       | AniList OAuth client ID ([create here](https://anilist.co/settings/developer)) |
| `ANILIST_CLIENT_SECRET` | No       | AniList OAuth client secret                                  |
| `MAL_CLIENT_ID`         | No       | MyAnimeList OAuth client ID ([create here](https://myanimelist.net/apiv2/team/settings)) |
| `MAL_CLIENT_SECRET`     | No       | MyAnimeList OAuth client secret                              |
| `SHIKIMORI_CLIENT_ID`   | No       | Shikimori OAuth client ID ([create here](https://shikimori.one/settings/apps)) |
| `SHIKIMORI_CLIENT_SECRET` | No    | Shikimori OAuth client secret                                |
| `BANGUMI_CLIENT_ID`     | No       | Bangumi OAuth client ID ([create here](https://bgm.tv/dev/app/create)) |
| `BANGUMI_CLIENT_SECRET` | No       | Bangumi OAuth client secret                                  |
| `MANGABAKA_CLIENT_ID`   | No       | MangaBaka OAuth client ID                                    |
| `MANGABAKA_CLIENT_SECRET` | No    | MangaBaka OAuth client secret                                |

### OAuth Redirect URIs

Configure these as the allowed redirect URIs in each service's OAuth app settings:

| Service    | Redirect URI                                         |
| ---------- | ---------------------------------------------------- |
| AniList    | `https://<your-deploy>/oauth/anilist/callback`       |
| MAL        | `https://<your-deploy>/oauth/myanimelist/callback`   |
| Shikimori  | `https://<your-deploy>/oauth/shikimori/callback`     |
| Bangumi    | `https://<your-deploy>/oauth/bangumi/callback`       |
| MangaBaka  | `https://<your-deploy>/oauth/mangabaka/callback`     |

Session ID is passed via the `state` OAuth parameter (not in the URL path).

## Notes on the Cloudflare migration

- **KV store** — `Deno.openKv()` is replaced by a single Durable Object. It was
  chosen over Workers KV because pairing codes and OAuth sessions need
  read-after-write consistency: the Telegram webhook writes the result while a
  KOReader device reads it back from another edge location, and KV can serve a
  stale value for up to a minute. Durable Object storage has no native TTL, so
  `src/kv/do.ts` stores an expiry per entry, drops it lazily on read, and sweeps
  the rest with an alarm.
- **Environment** — Cloudflare injects bindings into `fetch` instead of exposing
  `Deno.env`. `src/worker.ts` and `main.ts` publish them via `useEnv()`, which
  keeps the config helpers synchronous and unchanged.
- **Turso** — `@libsql/client/web` replaces the Node build. `libsql://` URLs are
  rewritten to `https://` so the client stays on the fetch transport, which
  works on every runtime.
- **Bot delivery** — Workers cannot long-poll, so the deployed Worker only ever
  uses webhooks (`src/worker.ts`); long polling lives in `main.ts` for local
  development.

## API Reference

OpenAPI spec available at `GET /doc` when the server is running.

### Cookie Sync Endpoints

| Method | Path                                                    | Description                    |
| ------ | ------------------------------------------------------- | ------------------------------ |
| GET    | `/api/pairing/generate`                                 | Generate 8-char pairing code   |
| GET    | `/api/pairing/status?code=`                             | Check if code was claimed      |
| GET    | `/api/cookie/get?chat_id=&device=&domain=`              | Get cookies for domain         |
| GET    | `/api/cookie/devices?chat_id=`                          | List linked devices            |
| GET    | `/api/cookie/sync-all?chat_id=&device=`                 | Bulk sync all domains          |
| GET    | `/api/cookie/notify-needs-update?chat_id=&device=&url=` | Notify user of expired cookies |
| GET    | `/api/webapp/data?initData=&device=`                    | WebApp data endpoint           |
| POST   | `/api/webapp/cookies`                                   | Ingest cookies from WebApp     |
| POST   | `/api/webapp/clear`                                     | Clear cookies                  |
| POST   | `/api/webapp/unlink`                                    | Unlink device                  |

### OAuth Bridge Endpoints

| Method | Path                                      | Description                        |
| ------ | ----------------------------------------- | ---------------------------------- |
| POST   | `/api/oauth/session`                      | Create OAuth session (returns QR)  |
| GET    | `/api/oauth/status/:sessionId`            | Poll session status                |
| GET    | `/oauth/:service/:sessionId`              | Bridge page (scan QR to sign in)   |
| GET    | `/oauth/anilist/callback`                 | AniList OAuth callback             |
| GET    | `/oauth/myanimelist/callback`             | MAL OAuth callback                 |
| GET    | `/oauth/shikimori/callback`               | Shikimori OAuth callback           |
| GET    | `/oauth/bangumi/callback`                 | Bangumi OAuth callback             |
| GET    | `/oauth/mangabaka/callback`               | MangaBaka OAuth callback           |
