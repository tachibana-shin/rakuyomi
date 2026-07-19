import { OpenAPIHono } from "@hono/zod-openapi"
import { createRoute, z } from "@hono/zod-openapi"
import { createOAuthSession, getOAuthSession } from "../../../../oauth_kv.ts"
import { generateCodeVerifier, generateCodeChallenge } from "../../../../logic/pkce.ts"
import { oauthServiceSchema } from "../../../../schemas.ts"

const CreateSessionBody = z.object({
  service: oauthServiceSchema.openapi({
    example: "anilist",
  }),
  chat_id: z.number().optional().openapi({ example: 123456789 }),
  device_name: z.string().optional().openapi({ example: "kindle_bedroom" }),
})

const CreateSessionResponse = z.object({
  session_id: z.string().openapi({ example: "a1b2c3d4" }),
  bridge_path: z.string().openapi({ example: "/oauth/anilist/a1b2c3d4" }),
  pkce_challenge: z.string().nullable().optional(),
})

const route = createRoute({
  method: "post",
  path: "/api/oauth/session",
  tags: ["OAuth"],
  description: "Create a new OAuth session for tracking sign-in. Returns a bridge path for QR code display.",
  request: {
    body: {
      content: { "application/json": { schema: CreateSessionBody } },
      required: true,
    },
  },
  responses: {
    200: {
      content: {
        "application/json": { schema: CreateSessionResponse },
      },
      description: "Session created",
    },
  },
})

const app = new OpenAPIHono()

app.openapi(route, async (c) => {
  const { service, chat_id, device_name } = c.req.valid("json")

  const chars = "abcdefghijklmnopqrstuvwxyz0123456789"
  const bytes = new Uint8Array(8)
  crypto.getRandomValues(bytes)
  let sessionId = ""
  for (let i = 0; i < 8; i++) {
    sessionId += chars.charAt(bytes[i] % chars.length)
  }

  const pkceVerifier = generateCodeVerifier()
  const pkceChallenge = await generateCodeChallenge(pkceVerifier)

  await createOAuthSession(sessionId, service, {
    chat_id,
    device_name,
  })

  const session = await getOAuthSession(sessionId)
  if (session) {
    const { getKv } = await import("../../../../kv.ts")
    const kv = await getKv()
    await kv.set(
      ["oauth", sessionId],
      { ...session, pkce_verifier: pkceVerifier, pkce_challenge: pkceChallenge },
      { expireIn: 10 * 60 * 1000 },
    )
  }

  const bridgePath = `/oauth/${service}/${sessionId}`

  return c.json({
    session_id: sessionId,
    bridge_path: bridgePath,
    pkce_challenge: pkceChallenge,
  })
})

export default app
