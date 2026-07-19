import { OpenAPIHono } from "@hono/zod-openapi"
import { createRoute, z } from "@hono/zod-openapi"
import { getDeviceCookies, getDeviceHash } from "../../../../store.ts"

const SyncAllQuery = z.object({
  chat_id: z.coerce.number().openapi({ example: 123456789 }),
  device: z.string().min(1).openapi({ example: "/kindle" }),
  hash: z.string().optional().openapi({ example: "abc123" }),
})

const SyncAllChangedResponse = z.object({
  status: z.literal("success"),
  changed: z.literal(true),
  hash: z.string().nullable(),
  payload: z.record(
    z.string(),
    z.object({
      cookies: z.array(z.unknown()),
      user_agent: z.string().optional(),
    }),
  ),
})

const SyncAllNotChangedResponse = z.object({
  status: z.literal("success"),
  changed: z.literal(false),
  hash: z.string().nullable(),
})

const route = createRoute({
  method: "get",
  path: "/api/cookie/sync-all",
  tags: ["Cookie"],
  description: "Sync all cookies for a device (returns payload if hash changed)",
  request: { query: SyncAllQuery },
  responses: {
    200: {
      content: {
        "application/json": {
          schema: z.union([SyncAllChangedResponse, SyncAllNotChangedResponse]),
        },
      },
      description: "Cookie sync result",
    },
  },
})

const app = new OpenAPIHono()

app.openapi(route, async (c) => {
  const { chat_id, device, hash } = c.req.valid("query")

  const currentHash = await getDeviceHash(chat_id, device)
  if (hash && currentHash && hash === currentHash) {
    return c.json({ status: "success", changed: false, hash: currentHash })
  }

  const globalData = device !== "/all"
    ? await getDeviceCookies(chat_id, "/all")
    : new Map()
  const deviceData = await getDeviceCookies(chat_id, device)

  const payload: Record<string, { cookies: unknown[]; user_agent?: string }> = {}

  for (const [domain, data] of globalData) {
    payload[domain] = { cookies: data.cookies, user_agent: data.user_agent }
  }

  for (const [domain, data] of deviceData) {
    payload[domain] = { cookies: data.cookies, user_agent: data.user_agent }
  }

  return c.json({
    status: "success",
    changed: true,
    hash: currentHash,
    payload,
  })
})

export default app
