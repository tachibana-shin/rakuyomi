import { OpenAPIHono } from "@hono/zod-openapi"
import { createRoute, z } from "@hono/zod-openapi"
import { getBot } from "../../../../bot/shared.ts"
import { t } from "../../../../i18n.ts"
import { getDevices } from "../../../../store.ts"

const NotifyQuery = z.object({
  chat_id: z.coerce.number().openapi({ example: 123456789 }),
  device: z.string().min(1).openapi({ example: "/kindle" }),
  url: z.string().optional().openapi({ example: "https://example.com/manga" }),
})

const NotifySuccessResponse = z.object({
  status: z.literal("success"),
})

const NotifySkippedResponse = z.object({
  status: z.literal("skipped"),
  reason: z.literal("device_not_linked"),
})

const route = createRoute({
  method: "get",
  path: "/api/cookie/notify-needs-update",
  tags: ["Cookie"],
  description: "Send a Telegram notification that cookies need updating",
  request: { query: NotifyQuery },
  responses: {
    200: {
      content: {
        "application/json": {
          schema: z.union([NotifySuccessResponse, NotifySkippedResponse]),
        },
      },
      description: "Notification result",
    },
  },
})

const app = new OpenAPIHono()

app.openapi(route, async (c) => {
  const { chat_id, device, url } = c.req.valid("query")

  const bot = getBot()
  const devices = await getDevices(chat_id)
  const locale = await t(chat_id)

  if (!devices.includes(device) && !devices.includes("/all")) {
    return c.json({ status: "skipped", reason: "device_not_linked" })
  }

  await bot.api.sendMessage(
    chat_id,
    locale.cookie_needs_update(device, url ?? ""),
  )
  return c.json({ status: "success" })
})

export default app
