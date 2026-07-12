import { Context } from "grammy"
import {
  clearAllCookies,
  clearDeviceCookies,
  clearDeviceDomainCookies,
  getDevices,
} from "../../store.ts"
import { t } from "../../i18n.ts"

export async function clearcookiesCommand(ctx: Context) {
  const chatId = ctx.chat?.id
  if (!chatId) return

  const args = ctx.message?.text?.split(/\s+/).slice(1) ?? []
  const deviceName = args[0]
  const domain = args[1]

  if (deviceName && domain) {
    const ok = await clearDeviceDomainCookies(chatId, deviceName, domain)
    if (ok) {
      await ctx.reply(t(chatId).clearcookies_domain_done(domain, deviceName))
    } else {
      await ctx.reply(t(chatId).clearcookies_none)
    }
  } else if (deviceName) {
    const ok = await clearDeviceCookies(chatId, deviceName)
    if (ok) {
      await ctx.reply(t(chatId).clearcookies_device_done(deviceName))
    } else {
      await ctx.reply(t(chatId).clearcookies_none)
    }
  } else {
    const devices = await getDevices(chatId)
    if (devices.length === 0) {
      await ctx.reply(t(chatId).clearcookies_none)
      return
    }
    await clearAllCookies(chatId)
    await ctx.reply(t(chatId).clearcookies_all_done)
  }
}
