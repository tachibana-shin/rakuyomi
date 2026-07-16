import { Context } from "grammy"
import { t } from "../../i18n.ts"
import { getDevices } from "../../store.ts"
import { getChatId } from "./utils.ts"

export async function devicesCommand(ctx: Context) {
  const chatId = getChatId(ctx)
  if (!chatId) return
  const locale = await t(chatId)

  const allDevices = await getDevices(chatId)
  const devices = allDevices.filter((d) => d !== "/all")

  if (devices.length === 0) {
    await ctx.reply(locale.devices_none)
    return
  }

  const deviceList = devices.map((d) => `- ${d}`).join("\n")
  await ctx.reply(locale.devices_list(deviceList))
}
