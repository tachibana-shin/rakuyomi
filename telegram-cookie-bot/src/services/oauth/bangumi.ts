import { getOAuthConfigs } from "../../config.ts"

export async function exchangeBangumiCode(
  code: string,
  redirectUri: string,
): Promise<{ access_token: string; refresh_token?: string }> {
  const configs = getOAuthConfigs()
  const cfg = configs.bangumi
  const body = new URLSearchParams({
    grant_type: "authorization_code",
    client_id: cfg.client_id,
    client_secret: cfg.client_secret,
    redirect_uri: redirectUri,
    code,
  })
  const res = await fetch(cfg.token_url, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: body.toString(),
  })
  if (!res.ok) {
    const err = await res.text()
    throw new Error(`Bangumi token exchange failed: ${res.status} ${err}`)
  }
  const data = await res.json()
  return {
    access_token: data.access_token,
    refresh_token: data.refresh_token,
  }
}
