import { OpenAPIHono, z } from "@hono/zod-openapi"
import { zValidator } from "@hono/zod-validator"
import { getConfig } from "../../../../../config.ts"
import { verifyTelegramWebAppData } from "../../../../utils/telegram-webapp.ts"
import { ingestCookies } from "../../../../store.ts"

const app = new OpenAPIHono()

const WebappCookiesBody = z.object({
  initData: z.string().min(1),
  device: z.string().min(1),
  cookies: z.string().min(1),
  user_agent: z.string().optional(),
})

app.post(
  "/api/webapp/cookies",
  zValidator("json", WebappCookiesBody),
  async (c) => {
    const { initData, device, cookies, user_agent } = c.req.valid("json")

    const { BOT_TOKEN } = getConfig()
    const result = await verifyTelegramWebAppData(initData, BOT_TOKEN)
    if (!result) {
      return c.json({ error: "Invalid initData" }, 403)
    }

    const chatId = result.userId
    const domains = ingestCookies(chatId, device, cookies, user_agent)

    if (domains.length === 0) {
      return c.json({ error: "Invalid cookie JSON format" }, 400)
    }

    return c.json({ ok: true, domains, device })
  },
)

export default app
