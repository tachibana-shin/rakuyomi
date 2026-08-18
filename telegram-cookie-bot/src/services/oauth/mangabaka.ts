import { getOAuthConfigs } from "../../config.ts"

export async function exchangeMangabakaCode(
  code: string,
  redirectUri: string,
  codeVerifier: string,
): Promise<{ access_token: string; refresh_token?: string }> {
  const configs = getOAuthConfigs()
  const cfg = configs.mangabaka
  const body = new URLSearchParams({
    grant_type: "authorization_code",
    client_id: cfg.client_id,
    client_secret: cfg.client_secret,
    redirect_uri: redirectUri,
    code,
    code_verifier: codeVerifier,
  })
  const res = await fetch(cfg.token_url, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: body.toString(),
  })
  if (!res.ok) {
    const err = await res.text()
    throw new Error(`MangaBaka token exchange failed: ${res.status} ${err}`)
  }
  const data = await res.json()
  return {
    access_token: data.access_token,
    refresh_token: data.refresh_token,
  }
}
