import { getOAuthConfigs } from "../../config.ts";

export async function exchangeShikimoriCode(
  code: string,
  redirectUri: string,
  codeVerifier?: string,
): Promise<{ access_token: string; refresh_token?: string }> {
  const configs = getOAuthConfigs();
  const cfg = configs.shikimori;
  const body: Record<string, string> = {
    grant_type: "authorization_code",
    client_id: cfg.client_id,
    client_secret: cfg.client_secret,
    code,
    redirect_uri: redirectUri,
  };
  if (codeVerifier) body.code_verifier = codeVerifier;
  const res = await fetch(cfg.token_url, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "User-Agent": "RakuYomi/1.0",
    },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const err = await res.text();
    throw new Error(`Shikimori token exchange failed: ${res.status} ${err}`);
  }
  const data = await res.json();
  return {
    access_token: data.access_token,
    refresh_token: data.refresh_token,
  };
}
