import { OpenAPIHono, z } from "@hono/zod-openapi"
import { zValidator } from "@hono/zod-validator"
import { getConfig } from "../../../../../config.ts"
import { verifyTelegramWebAppData } from "../../../../utils/telegram-webapp.ts"
import {
  clearAllCookies,
  clearDeviceCookies,
  clearDeviceDomainCookies,
} from "../../../../store.ts"

const app = new OpenAPIHono()

const WebappClearBody = z.object({
  initData: z.string().min(1),
  device: z.string().optional(),
  domain: z.string().optional(),
})

app.post(
  "/api/webapp/clear",
  zValidator("json", WebappClearBody),
  async (c) => {
    const { initData, device, domain } = c.req.valid("json")

    const { BOT_TOKEN } = getConfig()
    const result = await verifyTelegramWebAppData(initData, BOT_TOKEN)
    if (!result) {
      return c.json({ error: "Invalid initData" }, 403)
    }

    const chatId = result.userId

    if (device && domain) {
      await clearDeviceDomainCookies(chatId, device, domain)
    } else if (device) {
      await clearDeviceCookies(chatId, device)
    } else {
      await clearAllCookies(chatId)
    }

    return c.json({ ok: true })
  },
)

export default app
