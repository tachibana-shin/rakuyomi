import { OpenAPIHono, z } from "@hono/zod-openapi"
import { zValidator } from "@hono/zod-validator"
import { getConfig } from "../../../../config.ts"
import { verifyTelegramWebAppData } from "../../../../utils/telegram-webapp.ts"
import { getDeviceCookies, getDevices } from "../../../../store.ts"

const app = new OpenAPIHono()

const WebappDataQuery = z.object({
  initData: z.string().min(1),
  device: z.string().optional(),
})

app.get(
  "/api/webapp/data",
  zValidator("query", WebappDataQuery),
  async (c) => {
    const { initData, device } = c.req.valid("query")

    const { BOT_TOKEN } = getConfig()
    const result = await verifyTelegramWebAppData(initData, BOT_TOKEN)
    if (!result) {
      return c.json({ error: "Invalid initData" }, 403)
    }

    const chatId = result.userId

    const activeDevices = await getDevices(chatId)
    const activeDevice = device || activeDevices[0] || ""
    const payload: Record<string, { cookies: unknown[]; user_agent?: string }> =
      {}

    if (activeDevice) {
      const deviceData = await getDeviceCookies(chatId, activeDevice)
      for (const [domain, data] of deviceData) {
        payload[domain] = { cookies: data.cookies, user_agent: data.user_agent }
      }
    }

    return c.json({ devices: activeDevices, payload })
  },
)

export default app
