/**
 * Cloudflare Workers bindings plus the ambient request environment.
 *
 * Deno Deploy exposed configuration through `Deno.env`; Cloudflare instead
 * injects it into the `fetch` handler of the Worker. Both entry points
 * (`src/worker.ts` for production, `main.ts` for local development) publish
 * their bindings through `useEnv()` before any request is handled, so the
 * config helpers below can keep their synchronous, module-level API.
 *
 * Every request handled by the same isolate receives the very same bindings
 * object, therefore caching the reference is safe even when several requests
 * are in flight at the same time.
 */

// NOTE: `DurableObjectNamespace` and the other Workers types are ambient
// globals contributed by `@cloudflare/workers-types`. Importing them from the
// `cloudflare:workers` module instead would fail outside a Worker, and this
// module is also loaded by the local development entry point.

/** Everything the app needs from its host environment. */
export interface Env {
  // --- Core configuration ---
  /** Telegram bot token from @BotFather. */
  BOT_TOKEN: string
  /** Public base URL of the deployment, used to build OAuth redirect URIs. */
  PUBLIC_URL: string
  /** Turso (libSQL) database URL, e.g. `libsql://db-org.turso.io`. */
  TURSO_DB_URL: string
  /** Turso database auth token. */
  TURSO_AUTH_TOKEN: string
  /** Set to `"true"` to register the Telegram webhook (the Worker default). */
  USE_WEBHOOK?: string
  /** Shared secret checked on `/webhook` (sent as `X-Telegram-Bot-Api-Secret-Token`). */
  WEBHOOK_SECRET?: string

  // --- OAuth clients, one pair per tracking service ---
  ANILIST_CLIENT_ID?: string
  ANILIST_CLIENT_SECRET?: string
  MAL_CLIENT_ID?: string
  MAL_CLIENT_SECRET?: string
  SHIKIMORI_CLIENT_ID?: string
  SHIKIMORI_CLIENT_SECRET?: string
  BANGUMI_CLIENT_ID?: string
  BANGUMI_CLIENT_SECRET?: string
  MANGABAKA_CLIENT_ID?: string
  MANGABAKA_CLIENT_SECRET?: string

  // --- Bindings ---
  /** Durable Object backing the ephemeral key/value store (see src/kv). */
  KV_DO?: DurableObjectNamespace
}

let activeEnv: Env | null = null

/**
 * Publishes the bindings for the request that is about to be handled.
 * Called once at the entry point, before `app.fetch`.
 */
export function useEnv(env: Env): void {
  activeEnv = env
}

/** Returns the bindings published by `useEnv()`. Throws when there are none. */
export function getEnv(): Env {
  if (!activeEnv) {
    throw new Error(
      "Environment is not initialized: call useEnv() from the entry point first",
    )
  }
  return activeEnv
}
