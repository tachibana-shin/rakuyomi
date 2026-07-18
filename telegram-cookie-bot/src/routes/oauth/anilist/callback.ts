import { Hono } from "hono"
import { getOAuthSession, completeOAuthSession, errorOAuthSession } from "../../../oauth_kv.ts"
import { exchangeAnilistCode } from "../../../services/oauth/anilist.ts"
import { error, success, validateSession, notifyTelegramBot } from "../../../utils/oauth.tsx"

const app = new Hono()

app.get("/oauth/anilist/callback", async (c) => {
  const sessionId = c.req.query("state")
  const code = c.req.query("code")

  if (!sessionId) return error(c, "Error", "Missing state parameter.")

  const session = await getOAuthSession(sessionId)
  const check = validateSession(session, "anilist")
  if (!check.ok) return error(c, "Error", "Invalid session.")
  if (!code) return error(c, "Error", "No authorization code received.")

  const publicUrl = Deno.env.get("PUBLIC_URL") ?? ""
  const redirectUri = `${publicUrl}/oauth/anilist/callback`

  try {
    const tokens = await exchangeAnilistCode(code, redirectUri)
    await completeOAuthSession(sessionId, { access_token: tokens.access_token })
    if (check.session.chat_id) {
      await notifyTelegramBot(check.session.chat_id, "AniList")
    }
    return success(c, "Signed In!", "AniList connected successfully. You can close this page.")
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e)
    console.error("AniList callback error:", msg)
    await errorOAuthSession(sessionId, msg)
    return error(c, "Error", `Failed to sign in with AniList: ${msg}`)
  }
})

export default app
