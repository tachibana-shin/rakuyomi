import { Context } from "grammy"

const COOKIE_PATTERN = /^(\S+)\s+(\[.*\]|\{.*)/s

export function isCookieMessage(
  text: string,
): { deviceName: string | null; rawJson: string } | null {
  const match = text.match(COOKIE_PATTERN)
  if (!match) return null
  return {
    deviceName: match[1],
    rawJson: match[2],
  }
}

export async function fetchChatMessages(
  ctx: Context,
  limit = 100,
): Promise<string[]> {
  const chatId = ctx.chat?.id
  if (!chatId) return []

  try {
    const messages: string[] = []
    let lastId: number | undefined

    for (let i = 0; i < 5; i++) {
      const opts: Record<string, unknown> = { limit: Math.min(limit, 100) }
      if (lastId) opts.offset = lastId + 1

      const updates = await ctx.api.getUpdates(opts)

      for (const update of updates) {
        const msg = update.message
        if (msg && msg.chat.id === chatId && msg.text) {
          messages.push(msg.text)
          lastId = update.update_id
        }
      }

      if (updates.length < 100) break
    }

    return messages
  } catch {
    return []
  }
}
