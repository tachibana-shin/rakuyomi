import { OpenAPIHono, z } from "@hono/zod-openapi"
import { zValidator } from "@hono/zod-validator"
import { getDeviceCookies } from "../../../../store.ts"

const app = new OpenAPIHono()

const GetCookieQuery = z.object({
  chat_id: z.coerce.number(),
  device: z.string().min(1),
  domain: z.string().min(1),
})

app.get(
  "/api/cookie/get",
  zValidator("query", GetCookieQuery),
  async (c) => {
    const { chat_id, device, domain } = c.req.valid("query")
    const cleanDomain = domain.replace(/^\./, "")
    const deviceData = await getDeviceCookies(chat_id, device)
    const data = deviceData.get(cleanDomain)

    if (!data) {
      return c.json({ status: "not_found", domain, cookies: [] })
    }

    return c.json({
      status: "success",
      domain: data.domain,
      user_agent: data.user_agent,
      cookies: data.cookies,
    })
  },
)

export default app
