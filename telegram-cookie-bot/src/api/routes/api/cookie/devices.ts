import { OpenAPIHono } from "@hono/zod-openapi"
import { createRoute, z } from "@hono/zod-openapi"
import { getDevices } from "../../../../store.ts"

const DevicesQuery = z.object({
  chat_id: z.coerce.number().openapi({ example: 123456789 }),
})

const DevicesResponse = z.object({
  devices: z.array(z.string()),
})

const route = createRoute({
  method: "get",
  path: "/api/cookie/devices",
  tags: ["Cookie"],
  description: "List linked devices for a chat",
  request: { query: DevicesQuery },
  responses: {
    200: {
      content: {
        "application/json": { schema: DevicesResponse },
      },
      description: "List of devices",
    },
  },
})

const app = new OpenAPIHono()

app.openapi(route, async (c) => {
  const { chat_id } = c.req.valid("query")
  return c.json({ devices: await getDevices(chat_id) })
})

export default app
