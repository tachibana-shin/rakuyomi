import { Hono } from "hono"
import { getOAuthSession, completeOAuthSession, errorOAuthSession } from "../../../oauth_kv.ts"
import { exchangeShikimoriCode } from "../../../services/oauth/shikimori.ts"
import { error, success, validateSession, notifyTelegramBot } from "../../../utils/oauth.tsx"

const app = new Hono()

app.get("/oauth/shikimori/callback", async (c) => {
  const sessionId = c.req.query("state")
  const code = c.req.query("code")

  if (!sessionId) return error(c, "Error", "Missing state parameter.")

  const session = await getOAuthSession(sessionId)
  const check = validateSession(session, "shikimori")
  if (!check.ok) return error(c, "Error", "Invalid session.")
  if (!code) return error(c, "Error", "No authorization code received.")
  if (!check.session.pkce_verifier) {
    return error(c, "Error", "PKCE verifier not found. Session may have expired.")
  }

  const publicUrl = Deno.env.get("PUBLIC_URL") ?? ""
  const redirectUri = `${publicUrl}/oauth/shikimori/callback`

  try {
    const tokens = await exchangeShikimoriCode(code, redirectUri, check.session.pkce_verifier)
    await completeOAuthSession(sessionId, {
      access_token: tokens.access_token,
      refresh_token: tokens.refresh_token,
    })
    if (check.session.chat_id) {
      await notifyTelegramBot(check.session.chat_id, "Shikimori")
    }
    return success(c, "Signed In!", "Shikimori connected successfully. You can close this page.")
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e)
    console.error("Shikimori callback error:", msg)
    await errorOAuthSession(sessionId, msg)
    return error(c, "Error", `Failed to sign in with Shikimori: ${msg}`)
  }
})

export default app
