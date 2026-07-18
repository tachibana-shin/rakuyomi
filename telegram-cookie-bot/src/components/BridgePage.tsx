import type { FC } from "hono/jsx"
import { getOAuthConfigs } from "../config.ts"
import type { OAuthService } from "../oauth_kv.ts"
import { OAUTH_SERVICE_NAMES, OAUTH_SERVICE_COLORS } from "../schemas.ts"

interface BridgePageProps {
  service: string
  sessionId: string
  pkceChallenge?: string
}

export const BridgePage: FC<BridgePageProps> = ({
  service,
  sessionId,
  pkceChallenge,
}) => {
  const svc = service as OAuthService
  const serviceName = OAUTH_SERVICE_NAMES[svc] ?? service
  const serviceColor = OAUTH_SERVICE_COLORS[svc] ?? "#2ea6ff"
  const cfg = getOAuthConfigs()[svc]
  const pkceRequired = cfg?.pkce_required ?? false

  let authorizeUrl = ""
  if (cfg) {
    const publicUrl = Deno.env.get("PUBLIC_URL") ?? ""
    const redirectUri = `${publicUrl}/oauth/${service}/callback`
    const params = new URLSearchParams({
      client_id: cfg.client_id,
      redirect_uri: redirectUri,
      response_type: "code",
      state: sessionId,
    })
    if (cfg.scope) params.set("scope", cfg.scope)
    if (pkceChallenge) {
      params.set("code_challenge", pkceChallenge)
      params.set("code_challenge_method", pkceRequired ? "S256" : "plain")
    }
    authorizeUrl = `${cfg.authorize_url}?${params.toString()}`
  }

  return (
    <html lang="en">
      <head>
        <meta charset="UTF-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1.0" />
        <title>RakuYomi - Sign in with {serviceName}</title>
        <style
          dangerouslySetInnerHTML={{
            __html: `
          * { margin: 0; padding: 0; box-sizing: border-box; }
          body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: #1a1a2e; color: #e0e0e0;
            display: flex; align-items: center; justify-content: center;
            min-height: 100vh; padding: 20px;
          }
          .card {
            background: #16162a; border-radius: 16px; padding: 32px 24px;
            max-width: 400px; width: 100%;
          }
          .header { text-align: center; margin-bottom: 24px; }
          .header h1 { font-size: 18px; font-weight: 700; margin-bottom: 4px; }
          .header p { font-size: 13px; color: #888; }
          .oauth-btn {
            display: block; width: 100%; padding: 14px 20px; border-radius: 12px;
            border: none; cursor: pointer; font-size: 15px; font-weight: 600;
            color: #fff; background: ${serviceColor};
            text-align: center; text-decoration: none;
            transition: opacity .2s;
          }
          .oauth-btn:hover { opacity: .85; }
          .status { text-align: center; margin-top: 16px; font-size: 13px; color: #888; }
          .status-ok { color: #4caf50; }
          .status-err { color: #e74c3c; }
        `,
          }}
        />
      </head>
      <body>
        <div class="card">
          <div class="header">
            <h1>Sign in with {serviceName}</h1>
            <p>RakuYomi tracking integration</p>
          </div>

          {authorizeUrl && (
            <a class="oauth-btn" href={authorizeUrl}>
              Login with {serviceName}
            </a>
          )}

          <div class="status" id="status"></div>
        </div>
      </body>
    </html>
  )
}
