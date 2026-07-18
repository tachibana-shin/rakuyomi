import { getConfig } from "./src/config.ts";
import { createBot, registerBotCommands } from "./src/bot/mod.ts";
import { setBot } from "./src/bot/shared.ts";
import { startPollingServer, startWebhookServer } from "./src/server.ts";

async function main() {
  const { BOT_TOKEN, USE_WEBHOOK, WEBHOOK_URL } = getConfig();

  if (BOT_TOKEN) {
    const bot = createBot();
    setBot(bot);
    registerBotCommands(bot);

    if (BOT_TOKEN && USE_WEBHOOK && WEBHOOK_URL) {
      await bot.api.setWebhook(`${WEBHOOK_URL}/webhook`);
      startWebhookServer(bot, WEBHOOK_URL);
    } else {
      startPollingServer(bot);
    }
  } else {
    startWebhookServer(null, WEBHOOK_URL);
  }
}

await main();
