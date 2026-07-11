import { Bot } from "grammy"

let botInstance: Bot | null = null

export function setBot(bot: Bot) {
  botInstance = bot
}

export function getBot(): Bot {
  if (!botInstance) {
    throw new Error("Bot not initialized yet")
  }
  return botInstance
}
