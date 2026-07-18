import { OpenAPIHono } from "@hono/zod-openapi"
import { createRoute, z } from "@hono/zod-openapi"
import { createPairingCode } from "../../../../kv.ts"

const PairingGenerateResponse = z.object({
  pairing_code: z.string().openapi({ example: "ABC12345" }),
})

const route = createRoute({
  method: "get",
  path: "/api/pairing/generate",
  tags: ["Pairing"],
  description: "Generate a new pairing code for linking a device",
  responses: {
    200: {
      content: {
        "application/json": { schema: PairingGenerateResponse },
      },
      description: "Pairing code generated",
    },
  },
})

const app = new OpenAPIHono()

app.openapi(route, async (c) => {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
  const bytes = new Uint8Array(8)
  crypto.getRandomValues(bytes)
  let code = ""
  for (let i = 0; i < 8; i++) {
    code += chars.charAt(bytes[i] % chars.length)
  }
  await createPairingCode(code)
  return c.json({ pairing_code: code })
})

export default app
