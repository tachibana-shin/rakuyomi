# Rakuyomi Cookie Sync Bot

Telegram bot + HTTP API for syncing browser cookies from Android (Kiwi Browser) to KOReader (Rakuyomi) devices.

## Architecture

- **Bot** — grammY, handles `/link`, `/devices`, cookie ingestion via text messages
- **API** — Hono + `@hono/zod-openapi`, serves cookie data to the Rust backend
- **DB** — Turso (libSQL), stores device registrations and per-device cookie JSON blobs
- **Webapp** — Telegram Mini App (Vue 3 SPA) for visual cookie management

```
User → Telegram → Deno Bot → Turso DB
KOReader → Rust Backend → Deno API → Turso DB
```

## Directory Structure

```
main.ts                # Entry: Deno.serve base → Hono API, /telegraf → grammY webhook
config.ts              # Env vars (BOT_TOKEN, TURSO_DB_URL, TURSO_AUTH_TOKEN, SERVER_URL)
src/
  server.ts            # Deno.serve + webhook setup
  turso.ts             # Turso client & migrations (devices + cookie_data tables)
  store.ts             # In-memory Map cache, synced to Turso
  kv.ts                # Pairing code storage (Deno KV / in-memory fallback)
  i18n.ts              # Locale resolver & per-chat language detection
  locales/             # en, vi, jp, zh-cn, zh-hk
  api/
    mod.ts             # OpenAPIHono app with /doc endpoint
    routes/
      health.ts
      webapp.tsx       # Server-rendered Vue 3 SPA
      api/
        pairing/generate.ts & status.ts
        cookie/get.ts, devices.ts, sync-all.ts, notify-needs-update.ts
        webapp/data.ts, cookies.ts, clear.ts, unlink.ts
  bot/
    mod.ts             # grammY bot factory
    shared.ts          # Bot singleton
    commands/          # start, help, link, unlink, devices, cookies, status, ...
  utils/
    schema.ts          # Zod schemas for cookie validation
    cookie.ts          # Cookie parsing utilities
    telegram-webapp.ts # initData verification
```

## Deployment

### Deno Deploy

1. Fork/deploy this bot directory to [Deno Deploy](https://dash.deno.com/).
2. Set entry point to `main.ts`.
3. Configure environment variables:

| Variable | Description |
|---|---|
| `BOT_TOKEN` | Telegram bot token from [@BotFather](https://t.me/BotFather) |
| `TURSO_DB_URL` | Turso database URL |
| `TURSO_AUTH_TOKEN` | Turso database auth token |

4. Set the bot webhook:
   ```
   https://api.telegram.org/bot<BOT_TOKEN>/setWebhook?url=https://<your-deploy>.deno.dev/telegraf
   ```

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

### Key Endpoints

| Method | Path | Description |
|---|---|---|
| GET | `/api/pairing/generate` | Generate 8-char pairing code |
| GET | `/api/pairing/status?code=` | Check if code was claimed |
| GET | `/api/cookie/get?chat_id=&device=&domain=` | Get cookies for domain |
| GET | `/api/cookie/devices?chat_id=` | List linked devices |
| GET | `/api/cookie/sync-all?chat_id=&device=` | Bulk sync all domains |
| GET | `/api/cookie/notify-needs-update?chat_id=&device=&url=` | Notify user of expired cookies |
| GET | `/api/webapp/data?initData=&device=` | WebApp data endpoint |
| POST | `/api/webapp/cookies` | Ingest cookies from WebApp |
| POST | `/api/webapp/clear` | Clear cookies |
| POST | `/api/webapp/unlink` | Unlink device |
