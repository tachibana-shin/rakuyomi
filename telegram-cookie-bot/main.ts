/**
 * Local development entry point.
 *
 * Runs the whole stack in one Node process: the Hono app plus grammY long
 * polling, which is what `deno task dev` used to do. Cloudflare Workers cannot
 * long-poll, so the deployed Worker uses webhooks instead and this file is not
 * part of the deployment bundle (see `src/worker.ts`).
 *
 * Usage:
 *
 *     bun run dev                        # API on :8788 plus long polling
 *     DEV_POLLING=false bun run dev      # API only, keeps the webhook intact
 *     PORT=9000 bun run dev
 */
import { serve } from "@hono/node-server"
import app from "./src/server.ts"
import { ensureBot } from "./src/bot/shared.ts"
import { registerBotCommands } from "./src/bot/mod.ts"
import { useEnv, type Env } from "./src/env.ts"
import type { Bot } from "grammy"

// Bun loads `.env` from the working directory into `process.env`, which is
// exactly what the Env shape expects.
useEnv(process.env as unknown as Env)

const port = Number(process.env.PORT ?? 8788)

async function main() {
  const bot = await ensureBot()
  if (bot && process.env.DEV_POLLING === "false") {
    console.log(
      "DEV_POLLING=false: not polling, so a deployed instance keeps " +
        "receiving the updates.",
    )
  } else if (bot) {
    // Long polling calls deleteWebhook on the bot, so Telegram stops
    // delivering updates to any other deployment until that one registers its
    // webhook again.
    console.warn(
      "Long polling will delete the bot's webhook; a deployed instance " +
        'has to re-register it. Run with DEV_POLLING=false to skip this.',
    )
    await warnAboutSharedToken(bot)
    await registerBotCommands(bot)
    bot.start({
      onStart: () => console.log("Bot running in polling mode."),
    })
  }

  serve({ fetch: app.fetch, port }, (info) => {
    console.log(`Cookie sync API listening on http://localhost:${info.port}`)
    console.log(`OpenAPI docs: http://localhost:${info.port}/doc`)
  })
}

/**
 * Checks whether this token is already used by a deployment. Long polling and a
 * registered webhook are mutually exclusive - Telegram only routes updates to
 * `getUpdates` while no webhook exists - so a deployed worker that keeps
 * re-registering its webhook will keep winning.
 */
async function warnAboutSharedToken(bot: Bot): Promise<void> {
  try {
    const { url } = await bot.api.getWebhookInfo()
    if (!url || /^https?:\/\/(localhost|127\.|0\.0\.0\.0)/.test(url)) return

    console.warn(
      `\nThis token is already serving a deployment at:\n  ${url}\n` +
        "That deployment re-registers its webhook every few minutes, so it\n" +
        "keeps taking the updates back from this process. Polling with the\n" +
        "production token is not going to work reliably - use a separate bot\n" +
        "token for local development (a second bot from @BotFather), or run\n" +
        "with DEV_POLLING=false to only serve the local API.\n",
    )
  } catch (e) {
    console.warn("Could not check the current webhook state:", e)
  }
}

await main()
