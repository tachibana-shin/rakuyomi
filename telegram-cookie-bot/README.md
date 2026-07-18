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
- **Webapp** — Telegram Mini App for visual cookie management

```
User -> Telegram -> Deno Bot -> Turso DB
KOReader -> Rust Backend -> Deno API -> Turso DB
KOReader -> Rust Backend -> Deno OAuth Bridge -> Tracking Services
```

## Directory Structure

```
main.ts                          # Entry point
src/
  config.ts                      # All env vars (getConfig + getOAuthConfigs)
  schemas.ts                     # Zod schemas + service constants
  server.ts                      # Route registration + Deno.serve
  oauth_kv.ts                    # OAuth session KV layer
  kv.ts                          # Pairing code KV (Deno KV / in-memory fallback)
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
    myanimelist.ts                       # MAL token exchange
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
      myanimelist/callback.ts            # MAL callback (state=sessionId)
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

## Deployment

### Deno Deploy

1. Fork/deploy this bot directory to [Deno Deploy](https://dash.deno.com/).
2. Set entry point to `main.ts`.
3. Configure environment variables:

| Variable               | Required | Description                                                  |
| ---------------------- | -------- | ------------------------------------------------------------ |
| `BOT_TOKEN`            | Yes      | Telegram bot token from [@BotFather](https://t.me/BotFather) |
| `TURSO_DB_URL`         | Yes      | Turso database URL                                           |
| `TURSO_AUTH_TOKEN`     | Yes      | Turso database auth token                                    |
| `PUBLIC_URL`           | Yes      | Public URL of the deployed server (for OAuth redirect URIs)  |
| `USE_WEBHOOK`          | No       | Set to `true` for webhook mode (default: polling)            |
| `ANILIST_CLIENT_ID`    | No       | AniList OAuth client ID ([create here](https://anilist.co/settings/developer)) |
| `ANILIST_CLIENT_SECRET`| No       | AniList OAuth client secret                                  |
| `MAL_CLIENT_ID`        | No       | MyAnimeList OAuth client ID ([create here](https://myanimelist.net/apiv2/team/settings)) |
| `MAL_CLIENT_SECRET`    | No       | MyAnimeList OAuth client secret                              |
| `SHIKIMORI_CLIENT_ID`  | No       | Shikimori OAuth client ID ([create here](https://shikimori.one/settings/apps)) |
| `SHIKIMORI_CLIENT_SECRET` | No     | Shikimori OAuth client secret                                |
| `BANGUMI_CLIENT_ID`    | No       | Bangumi OAuth client ID ([create here](https://bgm.tv/dev/app/create)) |
| `BANGUMI_CLIENT_SECRET`| No       | Bangumi OAuth client secret                                  |
| `MANGABAKA_CLIENT_ID`  | No       | MangaBaka OAuth client ID                                   |
| `MANGABAKA_CLIENT_SECRET` | No     | MangaBaka OAuth client secret                               |

4. Set the bot webhook (if using webhook mode):
   ```
   https://api.telegram.org/bot<BOT_TOKEN>/setWebhook?url=https://<your-deploy>.deno.dev/webhook
   ```

### OAuth Redirect URIs

Configure these as the allowed redirect URIs in each service's OAuth app settings:

| Service    | Redirect URI                                         |
| ---------- | ---------------------------------------------------- |
| AniList    | `https://<your-deploy>/oauth/anilist/callback`       |
| MAL        | `https://<your-deploy>/oauth/myanimelist/callback`           |
| Shikimori  | `https://<your-deploy>/oauth/shikimori/callback`     |
| Bangumi    | `https://<your-deploy>/oauth/bangumi/callback`       |
| MangaBaka  | `https://<your-deploy>/oauth/mangabaka/callback`     |

Session ID is passed via the `state` OAuth parameter (not in the URL path).

### Local Development

```sh
# Start with live reload
deno task dev

# Type check
deno task check

# Lint
deno task lint

# Run tests
deno task test
```

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
| GET    | `/oauth/myanimelist/callback`                     | MAL OAuth callback                 |
| GET    | `/oauth/shikimori/callback`               | Shikimori OAuth callback           |
| GET    | `/oauth/bangumi/callback`                 | Bangumi OAuth callback             |
| GET    | `/oauth/mangabaka/callback`               | MangaBaka OAuth callback           |
