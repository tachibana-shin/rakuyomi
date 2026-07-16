import { OpenAPIHono, z } from "@hono/zod-openapi"
import { zValidator } from "@hono/zod-validator"
import { getDevices } from "../../../../store.ts"

const app = new OpenAPIHono()

const DevicesQuery = z.object({
  chat_id: z.coerce.number(),
})

app.get(
  "/api/cookie/devices",
  zValidator("query", DevicesQuery),
  async (c) => {
    const { chat_id } = c.req.valid("query")
    return c.json({ devices: await getDevices(chat_id) })
  },
)

export default app
