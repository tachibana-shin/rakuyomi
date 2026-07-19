import type { OAuthService } from "./oauth_kv.ts";

// --- App config ---

export function getConfig() {
  const BOT_TOKEN = Deno.env.get("BOT_TOKEN") ?? "";

  const USE_WEBHOOK = Deno.env.get("USE_WEBHOOK") === "true";
  const PUBLIC_URL = Deno.env.get("PUBLIC_URL") ?? "";
  const WEBHOOK_URL = PUBLIC_URL;

  const TURSO_DB_URL = Deno.env.get("TURSO_DB_URL") ?? "";
  const TURSO_AUTH_TOKEN = Deno.env.get("TURSO_AUTH_TOKEN") ?? "";

  return {
    BOT_TOKEN,
    USE_WEBHOOK,
    WEBHOOK_URL,
    PUBLIC_URL,
    TURSO_DB_URL,
    TURSO_AUTH_TOKEN,
  };
}

// --- OAuth provider config ---

export interface OAuthProviderConfig {
  authorize_url: string;
  token_url: string;
  client_id: string;
  client_secret: string;
  scope?: string;
  pkce_required?: boolean;
}

let oauthConfigs: Record<OAuthService, OAuthProviderConfig> | null = null;

export function getOAuthConfigs(): Record<OAuthService, OAuthProviderConfig> {
  if (oauthConfigs) return oauthConfigs;
  oauthConfigs = {
    anilist: {
      authorize_url: "https://anilist.co/api/v2/oauth/authorize",
      token_url: "https://anilist.co/api/v2/oauth/token",
      client_id: Deno.env.get("ANILIST_CLIENT_ID") ?? "",
      client_secret: Deno.env.get("ANILIST_CLIENT_SECRET") ?? "",
    },
    myanimelist: {
      authorize_url: "https://myanimelist.net/v1/oauth2/authorize",
      token_url: "https://myanimelist.net/v1/oauth2/token",
      client_id: Deno.env.get("MAL_CLIENT_ID") ?? "",
      client_secret: Deno.env.get("MAL_CLIENT_SECRET") ?? "",
      pkce_required: true,
    },
    shikimori: {
      authorize_url: "https://shikimori.one/oauth/authorize",
      token_url: "https://shikimori.one/oauth/token",
      client_id: Deno.env.get("SHIKIMORI_CLIENT_ID") ?? "",
      client_secret: Deno.env.get("SHIKIMORI_CLIENT_SECRET") ?? "",
    },
    bangumi: {
      authorize_url: "https://bgm.tv/oauth/authorize",
      token_url: "https://bgm.tv/oauth/access_token",
      client_id: Deno.env.get("BANGUMI_CLIENT_ID") ?? "",
      client_secret: Deno.env.get("BANGUMI_CLIENT_SECRET") ?? "",
    },
    mangabaka: {
      authorize_url: "https://mangabaka.org/auth/oauth2/authorize",
      token_url: "https://mangabaka.org/auth/oauth2/token",
      client_id: Deno.env.get("MANGABAKA_CLIENT_ID") ?? "",
      client_secret: Deno.env.get("MANGABAKA_CLIENT_SECRET") ?? "",
      scope: "library.read library.write offline_access",
      pkce_required: true,
    },
  };
  return oauthConfigs!;
}
