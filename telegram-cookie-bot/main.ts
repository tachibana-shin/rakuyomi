import { getConfig } from "./config.ts"
import { createBot, registerBotCommands } from "./src/bot/mod.ts"
import { setBot } from "./src/bot/shared.ts"
import { startPollingServer, startWebhookServer } from "./src/server.ts"

async function main() {
  const { USE_WEBHOOK, WEBHOOK_URL } = getConfig()

  const bot = createBot()
  setBot(bot)
  registerBotCommands(bot)

  if (USE_WEBHOOK && WEBHOOK_URL) {
    await bot.api.setWebhook(`${WEBHOOK_URL}/webhook`)
    startWebhookServer(bot, WEBHOOK_URL)
  } else {
    startPollingServer(bot)
  }
}

await main()
