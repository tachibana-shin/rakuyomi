import { Bot, webhookCallback } from "grammy"
import { getConfig } from "../config.ts"
import { getLocale, LANG_MAP, SUPPORTED_LANGUAGES } from "../i18n.ts"
import { startCommand } from "./commands/start.ts"
import { helpCommand } from "./commands/help.ts"
import { githubCommand } from "./commands/github.ts"
import { donateCommand } from "./commands/donate.ts"
import { handleLanguageCallback, languageCommand } from "./commands/language.ts"
import { linkCommand } from "./commands/link.ts"
import { devicesCommand } from "./commands/devices.ts"
import { statusCommand } from "./commands/status.ts"
import { cookiesCommand } from "./commands/cookies.ts"
import { appCommand } from "./commands/app.ts"
import { clearcookiesCommand } from "./commands/clearcookies.ts"
import { unlinkCommand } from "./commands/unlink.ts"
import { handleTextMessage } from "./commands/handle_text.ts"

function buildCommands(
  lang: string,
): Array<{ command: string; description: string }> {
  const l = getLocale(lang)
  return [
    { command: "start", description: l.command_start },
    { command: "link", description: l.command_link },
    { command: "devices", description: l.command_devices },
    { command: "cookies", description: l.command_cookies },
    { command: "app", description: l.command_app },
    { command: "clearcookies", description: l.command_clearcookies },
    { command: "status", description: l.command_status },
    { command: "help", description: l.command_help },
    { command: "language", description: l.command_language },
    { command: "github", description: l.command_github },
    { command: "donate", description: l.command_donate },
    { command: "unlink", description: "Unlink a device" },
  ]
}

export function createBot(): Bot {
  const { BOT_TOKEN } = getConfig()
  if (!BOT_TOKEN) throw new Error("BOT_TOKEN not found")
  const bot = new Bot(BOT_TOKEN)

  // Default parse_mode for all outgoing messages
  bot.api.config.use((prev, method, payload) => {
    if (
      ["sendMessage", "editMessageText", "answerInlineQuery"].includes(method)
    ) {
      return prev(method, { ...payload, parse_mode: "HTML" })
    }
    return prev(method, payload)
  })

  bot.command("start", startCommand)
  bot.command("link", linkCommand)
  bot.command("devices", devicesCommand)
  bot.command("cookies", cookiesCommand)
  bot.command("app", appCommand)
  bot.command("clearcookies", clearcookiesCommand)
  bot.command("status", statusCommand)
  bot.command("help", helpCommand)
  bot.command("language", languageCommand)
  bot.command("github", githubCommand)
  bot.command("donate", donateCommand)
  bot.command("unlink", unlinkCommand)

  // Handle language selection callback
  bot.callbackQuery(/^lang:/, handleLanguageCallback)

  // Handle text messages (cookie pastes)
  bot.on("message:text", handleTextMessage)

  return bot
}

export async function registerBotCommands(bot: Bot) {
  for (const lang of SUPPORTED_LANGUAGES) {
    const code = LANG_MAP[lang.code]
    if (!code) continue
    for (let attempt = 0; attempt < 3; attempt++) {
      try {
        await bot.api.setMyCommands(buildCommands(lang.code), {
          language_code: code,
        })
        break
      } catch (e: unknown) {
        const err = e as {
          error_code?: number
          parameters?: { retry_after?: number }
        }
        if (err?.error_code === 429 && err?.parameters?.retry_after) {
          const wait = err.parameters.retry_after * 1000 + 500
          console.warn(`Rate limited for ${lang.code}, waiting ${wait}ms...`)
          await new Promise((r) => setTimeout(r, wait))
        } else {
          console.warn(`Failed to set commands for ${lang.code}:`, e)
          break
        }
      }
    }
    await new Promise((r) => setTimeout(r, 1000))
  }
}

export function getWebhookHandler(bot: Bot) {
  return webhookCallback(bot, "std/http")
}
