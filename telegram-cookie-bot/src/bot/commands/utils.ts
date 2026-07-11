import { Context } from "grammy"

export function getChatId(ctx: Context): number | undefined {
  return ctx.chat?.id
}
