## DETAILED SYSTEM FLOWS & PROTOCOLS

> **Validation pattern:** All API routes use `zValidator('query', z.object({...}))` from `@hono/zod-openapi` for type-safe parameter validation. See https://hono.dev/docs/guides/validation and https://hono.dev/examples/zod-openapi. Do NOT use manual `c.req.query()` + `parseInt()` — use `z.coerce.number()` and `c.req.valid('query')` instead.

---

### 1. DEVICE PAIRING PROTOCOL

Links a Kindle device to a Telegram chat using a short-lived 8-character code.

```
Kindle (KOReader)       Deno Bot Server          User (Telegram)
       |                       |                       |
       | 1. GET /pairing/generate                     |
       |---------------------->|                       |
       | 2. { pairing_code }   |                       |
       |<----------------------|                       |
       |                       |                       |
       | 3. User sees code on screen                   |
       |                       |                       |
       |                       |   4. /link CODE NAME   |
       |                       |<----------------------|
       |                       |                       |
       |                       | 5. registerDevice()   |
       |                       |    → Turso devices    |
       |                       |    → Turso cookie_data|
       |                       |                       |
       | 6. GET /pairing/status?code=...               |
       |---------------------->|                       |
       | 7. { paired, chat_id, device }               |
       |<----------------------|                       |
```

- Code stored in `kv.ts` (Deno KV or in-memory Map) with 5-min TTL.
- `registerDevice()` inserts into both `devices` and `cookie_data` tables.
- KOReader polls `/pairing/status` to detect when the user has sent `/link`.

---

### 2. COOKIE INGESTION PROTOCOL (Android → Telegram → Turso)

User exports cookies from browser and sends to bot.

```
Kiwi Browser          User (Telegram)         Deno Bot           Turso
     |                       |                    |                 |
     | 1. Solve Cloudflare   |                    |                 |
     | 2. Export cookies     |                    |                 |
     | via extension         |                    |                 |
     |                       | 3. Paste JSON      |                 |
     |                       |-------------------->|                 |
     |                       |                    | 4. parse JSON   |
     |                       |                    | 5. extract UA   |
     |                       |                    | 6. ingest()     |
     |                       |                    |----------------->|
     |                       |                    | 7. persistDevice|
     |                       |                    |----------------->|
     |                       | 8. ✅ confirmation  |                 |
     |                       |<--------------------|                 |
```

- Extension used: **Get cookies.txt LOCALLY** — exports JSON array.
- User can prefix with device name: `kindle_bedroom [{...}]`.
- User-Agent line can be before or after JSON: `Mozilla/5.0 ... [...cookies...]` or `[...cookies...] Mozilla/5.0 ...`.
- `handle_text.ts` parses, `ingestCookies()` merges into in-memory Map, `persistDevice()` serializes to Turso JSON blob.

---

### 3. COOKIE SYNC PROTOCOL (KOREADER → RUST → DENO)

Triggered when a WASM source request returns 403.

```
KOReader Lua     Rust Backend        Deno Bot          Turso
    |                |                   |                |
    | 1. source req  |                   |                |
    |--------------->|                   |                |
    |                | 2. 403            |                |
    |                |<-- (from source)  |                |
    |                |                   |                |
    |                | 3. sync_all()     |                |
    |                |------------------>|                |
    |                |                   | 4. load from   |
    |                |                   |--------------->|
    |                |                   | 5. device data |
    |                |                   |<---------------|
    |                | 6. merged payload |                |
    |                |<------------------|                |
    |                |                   |                |
    |                | 7. apply_cookies()|                |
    |                | 8. retry request  |                |
    |                |---> (to source)   |                |
    |                |                   |                |
    |                | 9a. 200 → done    |                |
    |                |  OR               |                |
    |                | 9b. still 403     |                |
    |                |    → notify()     |                |
    |                |------------------>|                |
    |                |                   | 10. sendMessage|
    |                |                   |---> (Telegram) |
```

- Step 3: `net.rs` calls `GET /api/cookie/sync-all?chat_id=&device=`.
- Step 6: Deno merges global `"/all"` cookies + device-specific overrides per domain.
- Step 9b: `notify_cookie_needs_update()` → `GET /api/cookie/notify-needs-update?chat_id=&device=&url=` → bot sends message with the failing URL/domain.

---

### 4. PERSISTENCE LAYER

```
store.ts (in-memory Map)
    │
    │  sync on every ingest / clear
    ▼
turso.ts
    │
    ├── devices (chat_id INTEGER, device TEXT)
    │     PRIMARY KEY (chat_id, device)
    │
    └── cookie_data (chat_id INTEGER, device TEXT, domains TEXT)
          PRIMARY KEY (chat_id, device)
          domains = JSON string: { "domain.tld": { "cookies": [...], "user_agent": "..." } }
```

- `persistDevice()`: serializes `Map → Object.fromEntries()` → writes JSON to `cookie_data.domains`.
- `loadDeviceData()`: reads from Turso, deserializes JSON into `Map<Domain, CookieData>`.
- Device list is read from `devices` table (not inferred from `cookie_data`).

---

### 5. WEBAPP FLOW (TELEGRAM MINI APP)

```
User opens /app       Deno Bot          Turso
     |                   |                |
     | 1. Open WebApp    |                |
     |------------------>|                |
     | 2. Server-rendered                 |
     |    Vue SPA page   |                |
     |<------------------|                |
     |                   |                |
     | 3. Fetch data     |                |
     |   GET /api/webapp/|                |
     |   data?initData=  |                |
     |------------------>|                |
     |                   | 4. Query       |
     |                   |--------------->|
     |                   | 5. Return      |
     |                   |<---------------|
     | 6. JSON payload   |                |
     |<------------------|                |
     |                   |                |
     | 7. User edits/    |                |
     |    adds cookies   |                |
     |   POST /api/webapp|                |
     |   /cookies        |                |
     |------------------>|                |
     |                   | 8. ingest +    |
     |                   |    persist     |
     |                   |--------------->|
```

- WebApp rendered server-side via Hono JSX (`webapp.tsx`).
- Vue 3 loaded from CDN for client-side reactivity.
- All API calls from webapp validated via Telegram WebApp initData.
