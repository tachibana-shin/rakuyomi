import { OpenAPIHono, z } from "@hono/zod-openapi"
import { zValidator } from "@hono/zod-validator"
import { verifyDeviceToken } from "../../../../turso.ts"
import { getDeviceCookies } from "../../../../store.ts"

const app = new OpenAPIHono()

const SyncAllQuery = z.object({
  chat_id: z.coerce.number(),
  device: z.string().min(1),
  token: z.string().min(1),
})

app.get(
  "/api/cookie/sync-all",
  zValidator("query", SyncAllQuery),
  async (c) => {
    const { chat_id, device, token } = c.req.valid("query")

    if (!await verifyDeviceToken(chat_id, device, token)) {
      return c.json({ status: "error", reason: "invalid_token" }, 403)
    }

    const globalData = device !== "/all"
      ? await getDeviceCookies(chat_id, "/all")
      : new Map()
    const deviceData = await getDeviceCookies(chat_id, device)

    const payload: Record<string, { cookies: unknown[]; user_agent?: string }> =
      {}

    for (const [domain, data] of globalData) {
      payload[domain] = { cookies: data.cookies, user_agent: data.user_agent }
    }

    for (const [domain, data] of deviceData) {
      payload[domain] = { cookies: data.cookies, user_agent: data.user_agent }
    }

    return c.json({ status: "success", payload })
  },
)

export default app
