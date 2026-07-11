# SYSTEM PROMPT: RAKUYOMI COOKIE SYNC BOT (DENO + TURSO)

## Objective
Build a Telegram Bot system deployed on **Deno** using TypeScript. The bot syncs Cloudflare cookies and User-Agents from an Android browser (Kiwi Browser) to multiple e-reader devices running KOReader (Rakuyomi plugin).

## Core Architecture
- **Database:** Turso (libSQL) with two tables:
  - `devices(chat_id INTEGER, device TEXT, PRIMARY KEY(chat_id, device))` — registered devices per chat
  - `cookie_data(chat_id INTEGER, device TEXT, domains TEXT, PRIMARY KEY(chat_id, device))` — JSON blob per device: `{ "domain.tld": { "cookies": [...], "user_agent": "..." } }`
- **Telegram Bot:** grammY with webhook + polling support
- **HTTP API:** Hono (see https://hono.dev/docs/api/routing and https://hono.dev/docs/api/context)
- **Validation:** `@hono/zod-openapi` — `zValidator` middleware for validating `query`, `param`, `json`, `form` with Zod schemas (see https://hono.dev/docs/guides/validation and https://hono.dev/examples/zod-openapi). Use `zValidator('query', z.object({...}))` instead of manual `c.req.query()` + `parseInt()`.
- **Server entry:** `Deno.serve()` routes base path to Hono, `/telegraf` path to grammY webhook via `webhookCallback`

## Directory Structure
```
main.ts                  # Entry: Deno.serve(fetch) routing
config.ts                # Env vars (BOT_TOKEN, TURSO_DB_URL, TURSO_AUTH_TOKEN, SERVER_URL)
src/
  server.ts              # Deno.serve setup, webhook + Hono routing
  turso.ts               # Turso client, migrations, DB helpers
  store.ts               # In-memory Map cache per chat, sync to Turso
  kv.ts                  # Pairing code storage (Deno KV or in-memory Map)
  i18n.ts                # Locale resolver (detect from chat)
  locales/
    en.ts, vi.ts, jp.ts, zh-cn.ts, zh-hk.ts
  api/
    mod.ts               # Hono app, middleware, route mounting
    routes/
      health.ts          # GET /health
      webapp.tsx          # GET /webapp/cookies (server-rendered Vue SPA)
      api/
        pairing/generate.ts  # GET /api/pairing/generate
        pairing/status.ts    # GET /api/pairing/status
        cookie/get.ts        # GET /api/cookie/get?chat_id&device&domain
        cookie/devices.ts    # GET /api/cookie/devices?chat_id
        cookie/sync-all.ts   # GET /api/cookie/sync-all?chat_id&device
        cookie/notify-needs-update.ts  # GET /api/cookie/notify-needs-update?chat_id&device&url
        webapp/data.ts      # GET  /api/webapp/data
        webapp/cookies.ts   # POST /api/webapp/cookies
        webapp/clear.ts     # POST /api/webapp/clear
        webapp/unlink.ts    # POST /api/webapp/unlink
  bot/
    mod.ts               # grammY bot factory, command registration
    shared.ts            # Bot singleton getter/setter
    commands/
      start.ts, help.ts, link.ts, unlink.ts,
      devices.ts, cookies.ts, status.ts, app.ts,
      clearcookies.ts, language.ts, handle_text.ts
  utils/
    schema.ts            # Zod schemas for cookie entries
    cookie.ts            # Cookie parsing utilities
    message.ts           # Telegram message formatting
    registry.ts          # Generic registry pattern
    telegram-webapp.ts   # WebApp initData verification
```

## Bot Commands

### `/link [CODE] [NAME]`
- Pair a device. User gets a pairing code from KOReader → Rakuyomi → Cookie Sync → Link Device.
- Validates code against in-memory cache (5-min TTL in `kv.ts`).
- Calls `registerDevice(chatId, device)` in Turso (`devices` table).
- Stores initial empty cookie data for the device.
- Reply: success message with device name.

### `/unlink [NAME]`
- Removes device from `devices` table and `cookie_data` table via Turso.
- Reply: confirmation.

### `/devices`
- Lists all linked devices by reading `devices` table from Turso.
- Filters out pseudo-device `"/all"`.
- Renders inline keyboard buttons.
- Reply: device list with status icons.

### `/cookies [DEVICE]`
- Shows stored cookie domains for a device via Turso query.
- Reply: list of domains stored per device.

### `/clearcookies [DEVICE] [DOMAIN]`
- Clears cookies for a device (or specific domain) via Turso.

### `/status`
- Shows bot health, DB connection status, registered device count.

### `/app`
- Opens Telegram Mini App webapp for cookie management.

### `/language` or `/lang`
- Switch bot interface language.

### `/start`, `/help`
- Welcome/help messages with instructions.

### Text handler (`handle_text.ts`)
- Detects JSON arrays in user messages via regex.
- Extracts optional device name prefix (first word before `[`).
- Parses cookies and optional User-Agent string (detects `Mozilla/...` or `User-Agent:` prefix).
- Stores via `store.ingestCookies(chatId, device, cookies, userAgent)`.
- Persists to Turso via `persistDevice()`.

## API Routes (Hono) — with `@hono/zod-openapi`

All API routes use **`zValidator` middleware** for type-safe validation instead of manual `c.req.query()` parsing. See https://hono.dev/docs/guides/validation for the `zValidator` API.

```ts
import { zValidator } from '@hono/zod-openapi'
import { z } from '@hono/zod-openapi'

// Example pattern:
app.get(
  '/api/cookie/get',
  zValidator(
    'query',
    z.object({
      chat_id: z.coerce.number(),
      device: z.string().min(1),
      domain: z.string().min(1),
    })
  ),
  (c) => {
    const { chat_id, device, domain } = c.req.valid('query')
    // ...
  }
)
```

For POST routes, use `zValidator('json', ...)` and `zValidator('param', ...)` for path params.

### `GET /api/pairing/generate`
- No params.
- Generates 8-char alphanumeric code, stores in `kv.ts` with 5-min TTL.
- Returns `{ "pairing_code": "A8F27K9X" }`.

### `GET /api/pairing/status?code=`
- Query: `code: z.string().min(1)`
- Checks if code was claimed (device registered).
- Returns `{ "status": "pending" }` or `{ "status": "paired", "chat_id": ..., "device": ... }`.

### `GET /api/cookie/get?chat_id=&device=&domain=`
- Query: `chat_id: z.coerce.number()`, `device: z.string().min(1)`, `domain: z.string().min(1)`
- Loads device data from Turso `cookie_data`.
- Returns `{ "status": "success", "domain": "...", "user_agent": "...", "cookies": [...] }`.
- Fallback: merges device-specific cookies with `"/all"` cookies per domain.

### `GET /api/cookie/devices?chat_id=`
- Query: `chat_id: z.coerce.number()`
- Returns `{ "devices": ["kindle_bedroom", ...] }` from Turso `devices` table.

### `GET /api/cookie/sync-all?chat_id=&device=`
- Query: `chat_id: z.coerce.number()`, `device: z.string().min(1)`
- Loads device-specific data from Turso, merges with global `"/all"` data (device overrides global per domain).
- Returns `{ "status": "success", "payload": { "domain.tld": { "cookies": [...], "user_agent": "..." } } }`.

### `GET /api/cookie/notify-needs-update?chat_id=&device=&url=`
- Query: `chat_id: z.coerce.number()`, `device: z.string().min(1)`, `url: z.string().optional()`
- Called by Rust backend when retry still 403 after cookie sync.
- Validates device is linked via `getDevices()`.
- Sends Telegram message via `bot.api.sendMessage(chatId, locale.cookie_needs_update(device, url))`.
- Returns `{ "status": "success" }`.

### `POST /api/webapp/cookies`
- Body (JSON): `initData: z.string()`, `device: z.string()`, `cookies: CookieArraySchema`, `user_agent: z.string().optional()`
- Validates Telegram WebApp initData.
- Calls `ingestCookies()` + `persistDevice()` to save to Turso.
- Returns `{ "status": "success" }`.

### `POST /api/webapp/clear`
- Body (JSON): `initData: z.string()`, `device: z.string()`, `domain: z.string().optional()`
- Clears cookies for device/domain.

### `POST /api/webapp/unlink`
- Body (JSON): `initData: z.string()`, `device: z.string()`
- Removes device from Turso.

## End-to-End Cookie Sync Flow

1. **User registers device**: KOReader → Link Device → `GET /api/pairing/generate` → user sends `/link CODE NAME` → device saved in Turso.
2. **User sends cookies**: opens Kiwi Browser → solves Cloudflare → exports via **Get cookies.txt LOCALLY** extension → sends JSON array to bot → `handle_text.ts` stores in Turso.
3. **KOReader requests chapter**: Rust backend `net.rs` sends HTTP request → gets 403.
4. **Auto sync**: `net.rs` calls `sync_all_cookies()` → Rust HTTP `GET /api/cookie/sync-all` → applies cookies.
5. **Retry**: builds new request with synced cookies + User-Agent → sends.
6. **If still 403**: `net.rs` calls `notify_cookie_needs_update()` → Deno bot `GET /api/cookie/notify-needs-update` → sends Telegram message to user with the URL/domain context.
7. **User renews cookies**: exports fresh cookies → sends to bot → next request will pick them up.

## Key Design Decisions
- **All API routes use `zValidator` middleware** for query/body validation instead of manual `c.req.query()` + `parseInt()` + null checks. See https://hono.dev/docs/guides/validation.
- For OpenAPI spec generation, use `OpenAPIHono` from `@hono/zod-openapi` with `createRoute()` and `app.doc('/doc', {...})`. See https://hono.dev/examples/zod-openapi.
- In-memory `Map` cache per chat for fast reads (lazy-loaded from Turso).
- `"/all"` pseudo-device for domain-global cookies (never shown in device list, never in `devices` table).
- Device list comes from `devices` table, not inferred from `cookie_data`.
- No cookie jar on Rust side — cookies set via manual `Cookie` header per request.
- User-Agent stored per domain in `CookieStoreData`, sent as `User-Agent` header on retry.
- Rust uses `rustls` (no OpenSSL).
