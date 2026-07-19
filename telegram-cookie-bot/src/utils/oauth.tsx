import type { Context } from "hono"
import type { OAuthSession } from "../oauth_kv.ts"
import type { OAuthService } from "../schemas.ts"
import { getBot } from "../bot/shared.ts"
import { ResultPage } from "../components/ResultPage.tsx"

export function error(c: Context, title: string, message: string) {
  return c.html(<ResultPage title={title} message={message} ok={false} />)
}

export function success(c: Context, title: string, message: string) {
  return c.html(<ResultPage title={title} message={message} ok />)
}

export function validateSession(
  session: OAuthSession | null,
  expectedService: OAuthService,
): { ok: true; session: OAuthSession } | { ok: false } {
  if (!session || session.service !== expectedService) {
    return { ok: false }
  }
  return { ok: true, session }
}

export async function notifyTelegramBot(chatId: number, displayName: string): Promise<void> {
  try {
    const bot = getBot()
    await bot.api.sendMessage(
      chatId,
      `<b>RakuYomi</b>\n\n` +
        `Successfully signed in with <b>${displayName}</b>.\n` +
        `You can close this page now.`,
    )
  } catch (e) {
    console.warn("Failed to notify Telegram:", e)
  }
}
