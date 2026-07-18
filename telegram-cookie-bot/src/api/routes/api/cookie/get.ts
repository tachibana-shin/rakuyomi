import { OpenAPIHono } from "@hono/zod-openapi"
import { createRoute, z } from "@hono/zod-openapi"
import { getDeviceCookies } from "../../../../store.ts"

const GetCookieQuery = z.object({
  chat_id: z.coerce.number().openapi({ example: 123456789 }),
  device: z.string().min(1).openapi({ example: "/kindle" }),
  domain: z.string().min(1).openapi({ example: "example.com" }),
})

const CookieSchema = z.object({
  name: z.string(),
  value: z.string(),
  domain: z.string().optional(),
  path: z.string().optional(),
  expires: z.number().optional(),
  http_only: z.boolean().optional(),
  secure: z.boolean().optional(),
  same_site: z.string().optional(),
})

const GetCookieSuccessResponse = z.object({
  status: z.literal("success"),
  domain: z.string(),
  user_agent: z.string().nullable().optional(),
  cookies: z.array(CookieSchema),
})

const GetCookieNotFoundResponse = z.object({
  status: z.literal("not_found"),
  domain: z.string(),
  cookies: z.array(z.never()),
})

const route = createRoute({
  method: "get",
  path: "/api/cookie/get",
  tags: ["Cookie"],
  description: "Get cookies for a specific device and domain",
  request: { query: GetCookieQuery },
  responses: {
    200: {
      content: {
        "application/json": {
          schema: z.union([GetCookieSuccessResponse, GetCookieNotFoundResponse]),
        },
      },
      description: "Cookies for the domain",
    },
  },
})

const app = new OpenAPIHono()

app.openapi(route, async (c) => {
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
})

export default app
