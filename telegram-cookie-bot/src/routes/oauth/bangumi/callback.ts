import { Hono } from "hono"
import { getOAuthSession, completeOAuthSession, errorOAuthSession } from "../../../oauth_kv.ts"
import { exchangeBangumiCode } from "../../../services/oauth/bangumi.ts"
import { error, success, validateSession, notifyTelegramBot } from "../../../utils/oauth.tsx"

const app = new Hono()

app.get("/oauth/bangumi/callback", async (c) => {
  const sessionId = c.req.query("state")
  const code = c.req.query("code")

  if (!sessionId) return error(c, "Error", "Missing state parameter.")

  const session = await getOAuthSession(sessionId)
  const check = validateSession(session, "bangumi")
  if (!check.ok) return error(c, "Error", "Invalid session.")
  if (!code) return error(c, "Error", "No authorization code received.")

  const publicUrl = Deno.env.get("PUBLIC_URL") ?? ""
  const redirectUri = `${publicUrl}/oauth/bangumi/callback`

  try {
    const tokens = await exchangeBangumiCode(code, redirectUri)
    await completeOAuthSession(sessionId, {
      access_token: tokens.access_token,
      refresh_token: tokens.refresh_token,
    })
    if (check.session.chat_id) {
      await notifyTelegramBot(check.session.chat_id, "Bangumi")
    }
    return success(c, "Signed In!", "Bangumi connected successfully. You can close this page.")
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e)
    console.error("Bangumi callback error:", msg)
    await errorOAuthSession(sessionId, msg)
    return error(c, "Error", `Failed to sign in with Bangumi: ${msg}`)
  }
})

export default app
