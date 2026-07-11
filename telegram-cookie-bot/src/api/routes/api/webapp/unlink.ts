import { OpenAPIHono, z } from "@hono/zod-openapi"
import { zValidator } from "@hono/zod-validator"
import { getConfig } from "../../../../../config.ts"
import { verifyTelegramWebAppData } from "../../../../utils/telegram-webapp.ts"
import { clearDeviceCookies } from "../../../../store.ts"
import { removePairingByDevice } from "../../../../kv.ts"
import { removeDevice } from "../../../../turso.ts"

const app = new OpenAPIHono()

const WebappUnlinkBody = z.object({
  initData: z.string().min(1),
  device: z.string().min(1),
})

app.post(
  "/api/webapp/unlink",
  zValidator("json", WebappUnlinkBody),
  async (c) => {
    const { initData, device } = c.req.valid("json")

    const { BOT_TOKEN } = getConfig()
    const result = await verifyTelegramWebAppData(initData, BOT_TOKEN)
    if (!result) {
      return c.json({ error: "Invalid initData" }, 403)
    }

    const chatId = result.userId

    await clearDeviceCookies(chatId, device)
    await removeDevice(chatId, device)
    await removePairingByDevice(chatId, device)

    return c.json({ ok: true })
  },
)

export default app
