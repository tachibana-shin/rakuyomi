import { Hono } from "hono"
import { createPairingCode } from "../../../../kv.ts"

const app = new Hono()

app.get("/api/pairing/generate", async (c) => {
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
