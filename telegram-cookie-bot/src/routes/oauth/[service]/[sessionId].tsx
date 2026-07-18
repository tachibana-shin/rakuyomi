import { Hono } from "hono"
import { getOAuthSession } from "../../../oauth_kv.ts"
import { BridgePage } from "../../../components/BridgePage.tsx"
import { oauthServiceSchema } from "../../../schemas.ts"

const app = new Hono()

app.get("/oauth/:service/:sessionId", async (c) => {
  const serviceParam = c.req.param("service")
  const parsed = oauthServiceSchema.safeParse(serviceParam)
  if (!parsed.success) {
    return c.html(
      <html>
        <body>
          <h1>Unknown service: {serviceParam}</h1>
        </body>
      </html>,
    )
  }
  const service = parsed.data
  const sessionId = c.req.param("sessionId")

  const session = await getOAuthSession(sessionId)
  if (!session) {
    return c.html(
      `<!DOCTYPE html><html><body><h1>Session expired or not found</h1><p>Please scan the QR code again from RakuYomi.</p></body></html>`,
    )
  }

  let pkceChallenge: string | undefined
  if (session.pkce_challenge) {
    pkceChallenge = session.pkce_challenge
  }

  return c.html(
    <BridgePage
      service={service}
      sessionId={sessionId}
      pkceChallenge={pkceChallenge}
    />,
  )
})

export default app
