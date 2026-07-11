import { OpenAPIHono, z } from "@hono/zod-openapi"
import { zValidator } from "@hono/zod-validator"
import { getDeviceCookies, getDeviceHash } from "../../../../store.ts"

const app = new OpenAPIHono()

const SyncAllQuery = z.object({
  chat_id: z.coerce.number(),
  device: z.string().min(1),
  hash: z.string().optional(),
})

app.get(
  "/api/cookie/sync-all",
  zValidator("query", SyncAllQuery),
  async (c) => {
    const { chat_id, device, hash } = c.req.valid("query")

    const currentHash = await getDeviceHash(chat_id, device)
    if (hash && currentHash && hash === currentHash) {
      return c.json({ status: "success", changed: false, hash: currentHash })
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

    return c.json({ status: "success", changed: true, hash: currentHash, payload })
  },
)

export default app
