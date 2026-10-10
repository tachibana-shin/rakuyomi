/**
 * Cloudflare Workers entry point.
 *
 * Receives the bindings Cloudflare injects into every request, publishes them
 * for the rest of the app, and then serves the Hono application. Registering
 * the Telegram webhook and the command list happens once per isolate in the
 * background, so the first request is not slowed down by it.
 */
import app from "./server.ts"
import { ensureBot } from "./bot/shared.ts"
import { registerBotCommands } from "./bot/mod.ts"
import { getConfig } from "./config.ts"
import { getEnv, useEnv, type Env } from "./env.ts"

// The `KV_DO` binding requires this class to be exported from the entrypoint.
export { KvDurableObject } from "./kv/do.ts"

export default {
  async fetch(
    request: Request,
    env: Env,
    ctx: ExecutionContext,
  ): Promise<Response> {
    useEnv(env)
    ctx.waitUntil(setupTelegram())
    return await app.fetch(request, env, ctx)
  },
} satisfies ExportedHandler<Env>

let setupDone = false

async function setupTelegram(): Promise<void> {
  if (setupDone) return
  setupDone = true
  try {
    const bot = await ensureBot()
    if (!bot) return

    const { PUBLIC_URL } = getConfig()
    if (!PUBLIC_URL) {
      console.warn("PUBLIC_URL is not set; skipping webhook registration")
    } else {
      // A Worker cannot long-poll, so webhooks are the only way Telegram can
      // deliver updates here. The secret token makes the endpoint reject
      // anything that did not come from Telegram.
      await bot.api.setWebhook(`${PUBLIC_URL}/webhook`, {
        secret_token: getEnv().WEBHOOK_SECRET,
      })
    }

    await registerBotCommands(bot)
  } catch (e) {
    // Do not cache the failure: the next request gets another chance.
    setupDone = false
    console.error("Telegram setup failed", e)
  }
}
