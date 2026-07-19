import { getOAuthConfigs } from "../../config.ts";

export async function exchangeMalCode(
  code: string,
  redirectUri: string,
  codeVerifier: string,
): Promise<{ access_token: string; refresh_token?: string }> {
  const configs = getOAuthConfigs();
  const cfg = configs.myanimelist;
  const body = new URLSearchParams({
    client_id: cfg.client_id,
    grant_type: "authorization_code",
    code,
    redirect_uri: redirectUri,
    code_verifier: codeVerifier,
  });
  if (cfg.client_secret) {
    body.set("client_secret", cfg.client_secret);
  }
  const res = await fetch(cfg.token_url, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: body.toString(),
  });
  if (!res.ok) {
    const err = await res.text();
    throw new Error(`MAL token exchange failed: ${res.status} ${err}`);
  }
  const data = await res.json();
  return {
    access_token: data.access_token,
    refresh_token: data.refresh_token,
  };
}
