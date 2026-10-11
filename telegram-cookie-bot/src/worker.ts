/**
 * Cloudflare Workers entry point.
 *
 * Receives the bindings Cloudflare injects into every request, publishes them
 * for the rest of the app, and then serves the Hono application.
 *
 * The webhook is re-asserted from here because `bun run dev` deletes it, but
 * nothing expensive happens on the request path: `registerBotCommands` lives in
 * `register-commands.ts` instead, because its five `setMyCommands` calls with a
 * one second pause between them turned every cold isolate into a multi-second
 * invocation, and commands only change when the code changes.
 */
import app from "./server.ts"
import { ensureBot } from "./bot/shared.ts"
import { getConfig } from "./config.ts"
import { getEnv, useEnv, type Env } from "./env.ts"

// The `KV_DO` binding requires this class to be exported from the entrypoint.
export { KvDurableObject } from "./kv/do.ts"

/** How often the webhook is re-asserted, in milliseconds. */
const WEBHOOK_REFRESH_MS = 5 * 60 * 1000

// Isolates come and go, so a one-shot flag would let a warm isolate serve
// requests while something else has deleted the webhook - starting local long
// polling does exactly that. Track when the last registration happened instead.
let lastWebhookAt = 0

export default {
  async fetch(
    request: Request,
    env: Env,
    ctx: ExecutionContext,
  ): Promise<Response> {
    useEnv(env)
    ctx.waitUntil(setupWebhook())
    return await app.fetch(request, env, ctx)
  },

  // No Cron Trigger is configured: the Free plan allows only 5 per account and
  // gives each of them 10 ms of CPU. This handler is what you run by hand with
  // `wrangler dev --test-scheduled` when local long polling has taken the
  // webhook and no traffic has arrived since. It costs nothing until a schedule
  // is declared in wrangler.toml.
  async scheduled(
    _controller: ScheduledController,
    env: Env,
    _ctx: ExecutionContext,
  ): Promise<void> {
    useEnv(env)
    await setupWebhook()
  },
} satisfies ExportedHandler<Env>

async function setupWebhook(): Promise<void> {
  const bot = await ensureBot()
  if (!bot) return

  const now = Date.now()
  if (now - lastWebhookAt < WEBHOOK_REFRESH_MS) return
  lastWebhookAt = now

  try {
    const { PUBLIC_URL } = getConfig()
    if (!PUBLIC_URL) {
      console.warn("PUBLIC_URL is not set; skipping webhook registration")
      return
    }
    // A Worker cannot long-poll, so webhooks are the only way Telegram can
    // deliver updates here. The secret token makes the endpoint reject
    // anything that did not come from Telegram.
    await bot.api.setWebhook(`${PUBLIC_URL}/webhook`, {
      secret_token: getEnv().WEBHOOK_SECRET,
    })
  } catch (e) {
    // Do not cache the failure: the next request gets another chance.
    lastWebhookAt = 0
    console.error("Webhook registration failed", e)
  }
}
