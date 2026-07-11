import { Bot, webhookCallback } from "grammy"
import { apiApp } from "./api/mod.ts"

export function startWebhookServer(bot: Bot, webhookUrl: string) {
  const webhookHandler = webhookCallback(bot, "std/http")

  apiApp.post("/webhook", async (c) => {
    return await webhookHandler(c.req.raw)
  })

  Deno.serve(apiApp.fetch)
  console.log(`Bot running in webhook mode: ${webhookUrl}/webhook`)
}

export function startPollingServer(bot: Bot) {
  bot.start()
  console.log("Bot running in polling mode. Press Ctrl+C to stop.")

  Deno.serve({ port: 8788 }, apiApp.fetch)
}
