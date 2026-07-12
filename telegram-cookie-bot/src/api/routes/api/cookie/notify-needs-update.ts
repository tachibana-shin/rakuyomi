import { OpenAPIHono, z } from "@hono/zod-openapi"
import { zValidator } from "@hono/zod-validator"
import { getBot } from "../../../../bot/shared.ts"
import { t } from "../../../../i18n.ts"
import { getDevices } from "../../../../store.ts"

const app = new OpenAPIHono()

const NotifyQuery = z.object({
  chat_id: z.coerce.number(),
  device: z.string().min(1),
  url: z.string().optional(),
})

app.get(
  "/api/cookie/notify-needs-update",
  zValidator("query", NotifyQuery),
  async (c) => {
    const { chat_id, device, url } = c.req.valid("query")

    const bot = getBot()
    const devices = await getDevices(chat_id)
    const locale = t(chat_id)

    if (!devices.includes(device) && !devices.includes("/all")) {
      return c.json({ status: "skipped", reason: "device_not_linked" })
    }

    await bot.api.sendMessage(
      chat_id,
      locale.cookie_needs_update(device, url ?? ""),
    )
    return c.json({ status: "success" })
  },
)

export default app
