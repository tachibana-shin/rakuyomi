/**
 * Holds the single bot instance shared by the webhook route and by the OAuth
 * callbacks, which notify the user through `notifyTelegramBot`.
 *
 * The bot is created lazily on first use because a Cloudflare Worker must not
 * do any work at import time - an isolate can be started by any request,
 * including ones that never touch Telegram.
 */
import type { Bot } from "grammy"
import { createBot } from "./mod.ts"
import { getConfig } from "../config.ts"

let botInstance: Bot | null = null
let pending: Promise<Bot | null> | null = null

/**
 * Creates the bot once per isolate. Resolves to `null` when no token is
 * configured, so the HTTP API keeps working without a bot.
 */
export function ensureBot(): Promise<Bot | null> {
  if (!pending) {
    pending = createBotOnce()
  }
  return pending
}

async function createBotOnce(): Promise<Bot | null> {
  try {
    const { BOT_TOKEN } = getConfig()
    if (!BOT_TOKEN) {
      console.warn("BOT_TOKEN is not set; bot features are disabled")
      return null
    }
    botInstance = createBot()
    return botInstance
  } catch (e) {
    // Let a later request try again instead of caching the failure.
    pending = null
    throw e
  }
}

/** Returns the bot, throwing when it has not been created yet. */
export function getBot(): Bot {
  if (!botInstance) {
    throw new Error("Bot not initialized yet")
  }
  return botInstance
}
