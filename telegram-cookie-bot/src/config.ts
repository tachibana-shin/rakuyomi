/**
 * Application configuration.
 *
 * On Deno Deploy these values came from `Deno.env`; on Cloudflare Workers they
 * are bindings injected into the request handler and published by `useEnv()`
 * (see `src/env.ts`). The shape of the returned objects is unchanged.
 */
import { getEnv } from "./env.ts"
import type { OAuthService } from "./oauth_kv.ts"

// --- App config ---

export function getConfig() {
  const env = getEnv()
  return {
    BOT_TOKEN: env.BOT_TOKEN ?? "",
    USE_WEBHOOK: env.USE_WEBHOOK === "true",
    WEBHOOK_URL: env.PUBLIC_URL ?? "",
    PUBLIC_URL: env.PUBLIC_URL ?? "",
    TURSO_DB_URL: env.TURSO_DB_URL ?? "",
    TURSO_AUTH_TOKEN: env.TURSO_AUTH_TOKEN ?? "",
  }
}

// --- OAuth provider config ---

export interface OAuthProviderConfig {
  authorize_url: string
  token_url: string
  client_id: string
  client_secret: string
  scope?: string
  pkce_required?: boolean
}

let oauthConfigs: Record<OAuthService, OAuthProviderConfig> | null = null

export function getOAuthConfigs(): Record<OAuthService, OAuthProviderConfig> {
  if (oauthConfigs) return oauthConfigs
  const env = getEnv()
  oauthConfigs = {
    anilist: {
      authorize_url: "https://anilist.co/api/v2/oauth/authorize",
      token_url: "https://anilist.co/api/v2/oauth/token",
      client_id: env.ANILIST_CLIENT_ID ?? "",
      client_secret: env.ANILIST_CLIENT_SECRET ?? "",
    },
    myanimelist: {
      authorize_url: "https://myanimelist.net/v1/oauth2/authorize",
      token_url: "https://myanimelist.net/v1/oauth2/token",
      client_id: env.MAL_CLIENT_ID ?? "",
      client_secret: env.MAL_CLIENT_SECRET ?? "",
      pkce_required: false,
    },
    shikimori: {
      authorize_url: "https://shikimori.one/oauth/authorize",
      token_url: "https://shikimori.one/oauth/token",
      client_id: env.SHIKIMORI_CLIENT_ID ?? "",
      client_secret: env.SHIKIMORI_CLIENT_SECRET ?? "",
    },
    bangumi: {
      authorize_url: "https://bgm.tv/oauth/authorize",
      token_url: "https://bgm.tv/oauth/access_token",
      client_id: env.BANGUMI_CLIENT_ID ?? "",
      client_secret: env.BANGUMI_CLIENT_SECRET ?? "",
    },
    // Endpoints and scopes come from MangaBaka's OIDC discovery document
    // (https://mangabaka.org/.well-known/openid-configuration). The registered
    // app is a public client (PKCE, no client secret), so
    // MANGABAKA_CLIENT_SECRET is intentionally unset - the token exchange sends
    // whatever is configured, which is an empty value here.
    mangabaka: {
      authorize_url: "https://mangabaka.org/auth/oauth2/authorize",
      token_url: "https://mangabaka.org/auth/oauth2/token",
      client_id: env.MANGABAKA_CLIENT_ID ?? "",
      client_secret: env.MANGABAKA_CLIENT_SECRET ?? "",
      scope: "openid profile library.read library.write offline_access",
      pkce_required: true,
    },
  }
  return oauthConfigs
}
