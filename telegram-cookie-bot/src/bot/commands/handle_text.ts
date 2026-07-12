import { Context } from "grammy"
import { t } from "../../i18n.ts"
import { CookieArraySchema } from "../../utils/schema.ts"
import { ingestCookies } from "../../store.ts"
import { getChatId } from "./utils.ts"

export async function handleTextMessage(ctx: Context) {
  const chatId = getChatId(ctx)
  if (!chatId) return
  const locale = t(chatId)

  const text = ctx.message?.text
  if (!text) return

  const jsonStart = text.indexOf("[")
  const jsonEnd = text.lastIndexOf("]")
  if (jsonStart === -1 || jsonEnd === -1 || jsonEnd <= jsonStart) return

  const rawJson = text.slice(jsonStart, jsonEnd + 1)
  const before = text.slice(0, jsonStart).trim()
  const after = text.slice(jsonEnd + 1).trim()

  let parsed: unknown
  try {
    parsed = JSON.parse(rawJson)
  } catch {
    await ctx.reply(locale.cookie_invalid_json)
    return
  }

  const result = CookieArraySchema.safeParse(parsed)
  if (!result.success) {
    await ctx.reply(locale.cookie_invalid_format)
    return
  }

  // Device name: first whitespace-delimited token that isn't a UA line
  const allMeta = [before, after].filter(Boolean).join("\n")
  const metaLines = allMeta.split("\n")
  let deviceName = "/all"
  let userAgent: string | null = null
  for (const line of metaLines) {
    const trimmed = line.trim()
    if (!trimmed) continue
    if (
      trimmed.startsWith("Mozilla/") || trimmed.startsWith("User-Agent:")
    ) {
      userAgent = trimmed.replace(/^User-Agent:\s*/i, "").trim()
    } else if (deviceName === "/all") {
      deviceName = trimmed.split(/\s+/)[0]
    }
  }

  const domains = await ingestCookies(
    chatId,
    deviceName,
    rawJson,
    userAgent ?? undefined,
  )
  if (domains.length === 0) return

  await ctx.reply(locale.cookie_received(domains.join(", "), deviceName))
}
