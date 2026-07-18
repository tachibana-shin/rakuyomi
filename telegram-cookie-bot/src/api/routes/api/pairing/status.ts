import { OpenAPIHono } from "@hono/zod-openapi"
import { createRoute, z } from "@hono/zod-openapi"
import { getPairingStatus } from "../../../../kv.ts"

const StatusQuery = z.object({
  code: z.string().min(1).openapi({ example: "ABC12345" }),
})

const StatusResponse = z.object({
  paired: z.boolean(),
  chat_id: z.number().nullable().optional(),
  device_name: z.string().nullable().optional(),
  api_token: z.string().nullable().optional(),
})

const route = createRoute({
  method: "get",
  path: "/api/pairing/status",
  tags: ["Pairing"],
  description: "Check the pairing status of a code",
  request: { query: StatusQuery },
  responses: {
    200: {
      content: {
        "application/json": { schema: StatusResponse },
      },
      description: "Pairing status",
    },
  },
})

const app = new OpenAPIHono()

app.openapi(route, async (c) => {
  const { code } = c.req.valid("query")
  const result = await getPairingStatus(code.toUpperCase())
  return c.json(result)
})

export default app
