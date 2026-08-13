import { OpenAPIHono } from "@hono/zod-openapi"
import { createRoute, z } from "@hono/zod-openapi"
import { getOAuthSession } from "../../../../../oauth_kv.ts"

const OAuthTokensSchema = z.object({
  access_token: z.string().nullable().optional(),
  refresh_token: z.string().nullable().optional(),
  client_id: z.string().nullable().optional(),
}).passthrough()

const StatusPendingResponse = z.object({
  status: z.literal("pending"),
  service: z.string(),
})

const StatusCompletedResponse = z.object({
  status: z.literal("completed"),
  service: z.string(),
  tokens: OAuthTokensSchema.nullable().optional(),
})

const StatusErrorResponse = z.object({
  status: z.literal("error"),
  service: z.string(),
  message: z.string(),
})

const StatusNotFoundResponse = z.object({
  status: z.literal("error"),
  message: z.string(),
})

const route = createRoute({
  method: "get",
  path: "/api/oauth/status/{sessionId}",
  tags: ["OAuth"],
  description: "Poll the OAuth session status (pending / completed / error)",
  request: {
    params: z.object({
      sessionId: z.string().openapi({ example: "a1b2c3d4" }),
    }),
  },
  responses: {
    200: {
      content: {
        "application/json": {
          schema: z.union([
            StatusPendingResponse,
            StatusCompletedResponse,
            StatusErrorResponse,
            StatusNotFoundResponse,
          ]),
        },
      },
      description: "Session status",
    },
  },
})

const app = new OpenAPIHono()

app.openapi(route, async (c) => {
  const { sessionId } = c.req.valid("param")
  const session = await getOAuthSession(sessionId)

  if (!session) {
    return c.json({ status: "error", message: "Session expired or not found" })
  }

  if (session.status === "completed") {
    return c.json({
      status: "completed",
      service: session.service,
      tokens: session.tokens,
    })
  }

  if (session.status === "error") {
    return c.json({
      status: "error",
      service: session.service,
      message: session.error_message ?? "Authentication failed",
    })
  }

  return c.json({ status: "pending", service: session.service })
})

export default app
