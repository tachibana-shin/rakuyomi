import { OpenAPIHono, z } from "@hono/zod-openapi"
import { zValidator } from "@hono/zod-validator"
import { getPairingStatus } from "../../../../kv.ts"

const app = new OpenAPIHono()

const StatusQuery = z.object({
  code: z.string().min(1),
})

app.get(
  "/api/pairing/status",
  zValidator("query", StatusQuery),
  async (c) => {
    const { code } = c.req.valid("query")
    const result = await getPairingStatus(code.toUpperCase())
    return c.json(result)
  },
)

export default app
