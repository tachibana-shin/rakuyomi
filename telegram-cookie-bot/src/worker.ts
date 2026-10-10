/**
 * Cloudflare Workers entry point.
 *
 * Receives the bindings Cloudflare injects into every request, publishes them
 * for the rest of the app, and then serves the Hono application. Registering
 * the Telegram webhook and the command list happens in the background, so no
 * request is slowed down by it.
 */
import app from "./server.ts"
import { ensureBot } from "./bot/shared.ts"
import { registerBotCommands } from "./bot/mod.ts"
import { getConfig } from "./config.ts"
import { getEnv, useEnv, type Env } from "./env.ts"

// The `KV_DO` binding requires this class to be exported from the entrypoint.
export { KvDurableObject } from "./kv/do.ts"

/** How often the webhook is (re)registered, in milliseconds. */
const WEBHOOK_REFRESH_MS = 5 * 60 * 1000
/** How often the command list is refreshed. It only changes when the bot does. */
const COMMANDS_REFRESH_MS = 24 * 60 * 60 * 1000

// Isolates are created and dropped constantly, so a one-shot flag would leave a
// warm isolate serving requests while something else has deleted the webhook -
// starting local long polling does exactly that, and the bot would stay offline
// until the isolate happens to restart. Track when each step last ran instead
// and repeat it once its interval has elapsed.
let lastWebhookAt = 0
let lastCommandsAt = 0

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

  // The refresh in `fetch` only runs when a request arrives, which stops being
  // true as soon as local long polling takes the webhook away and nobody opens
  // the app. The cron trigger keeps restoring it without any inbound traffic.
  async scheduled(
    _controller: ScheduledController,
    env: Env,
    _ctx: ExecutionContext,
  ): Promise<void> {
    useEnv(env)
    await setupTelegram()
  },
} satisfies ExportedHandler<Env>

async function setupTelegram(): Promise<void> {
  const bot = await ensureBot()
  if (!bot) return

  const now = Date.now()

  if (now - lastWebhookAt >= WEBHOOK_REFRESH_MS) {
    lastWebhookAt = now
    try {
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
    } catch (e) {
      // Do not cache the failure: the next request gets another chance.
      lastWebhookAt = 0
      console.error("Webhook registration failed", e)
    }
  }

  if (now - lastCommandsAt >= COMMANDS_REFRESH_MS) {
    lastCommandsAt = now
    try {
      await registerBotCommands(bot)
    } catch (e) {
      lastCommandsAt = 0
      console.error("Command registration failed", e)
    }
  }
}
