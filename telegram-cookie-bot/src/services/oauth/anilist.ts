import { getOAuthConfigs } from "../../config.ts"

export async function exchangeAnilistCode(
  code: string,
  redirectUri: string,
): Promise<{ access_token: string }> {
  const configs = getOAuthConfigs()
  const cfg = configs.anilist
  const body: Record<string, string> = {
    grant_type: "authorization_code",
    client_id: cfg.client_id,
    client_secret: cfg.client_secret,
    redirect_uri: redirectUri,
    code,
  }
  const res = await fetch(cfg.token_url, {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "application/json" },
    body: JSON.stringify(body),
  })
  if (!res.ok) {
    const err = await res.text()
    throw new Error(`AniList token exchange failed: ${res.status} ${err}`)
  }
  const data = await res.json()
  return { access_token: data.access_token }
}
