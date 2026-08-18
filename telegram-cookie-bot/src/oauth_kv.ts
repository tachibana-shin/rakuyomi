import { getKv } from "./kv.ts"
import type { OAuthService } from "./schemas.ts"

export type { OAuthService }

export interface OAuthTokens {
  access_token?: string
  refresh_token?: string
  // MAL requires client_id for search even after auth
  client_id?: string
}

export interface OAuthSession {
  service: OAuthService
  status: "pending" | "completed" | "error"
  tokens?: OAuthTokens
  // Telegram bot delivery
  chat_id?: number
  device_name?: string
  // PKCE
  pkce_verifier?: string
  pkce_challenge?: string
  created_at: number
  error_message?: string
}

const OAUTH_SESSION_TTL = 10 * 60 * 1000 // 10 minutes

export async function createOAuthSession(
  sessionId: string,
  service: OAuthService,
  options?: { chat_id?: number; device_name?: string },
): Promise<void> {
  const kv = await getKv()
  await kv.set(
    ["oauth", sessionId],
    {
      service,
      status: "pending",
      chat_id: options?.chat_id,
      device_name: options?.device_name,
      created_at: Date.now(),
    } satisfies OAuthSession,
    { expireIn: OAUTH_SESSION_TTL },
  )
}

export async function getOAuthSession(
  sessionId: string,
): Promise<OAuthSession | null> {
  const kv = await getKv()
  const res = await kv.get<OAuthSession>(["oauth", sessionId])
  if (!res.value) return null
  if (Date.now() - res.value.created_at >= OAUTH_SESSION_TTL) {
    await kv.delete(["oauth", sessionId])
    return null
  }
  return res.value
}

export async function completeOAuthSession(
  sessionId: string,
  tokens: OAuthTokens,
): Promise<boolean> {
  const kv = await getKv()
  const res = await kv.get<OAuthSession>(["oauth", sessionId])
  if (!res.value) return false
  const updated: OAuthSession = {
    ...res.value,
    status: "completed",
    tokens,
  }
  await kv.set(["oauth", sessionId], updated, { expireIn: OAUTH_SESSION_TTL })
  return true
}

export async function errorOAuthSession(
  sessionId: string,
  message: string,
): Promise<void> {
  const kv = await getKv()
  const res = await kv.get<OAuthSession>(["oauth", sessionId])
  if (!res.value) return
  const updated: OAuthSession = {
    ...res.value,
    status: "error",
    error_message: message,
  }
  await kv.set(["oauth", sessionId], updated, { expireIn: OAUTH_SESSION_TTL })
}

export async function deleteOAuthSession(sessionId: string): Promise<void> {
  const kv = await getKv()
  await kv.delete(["oauth", sessionId])
}

export async function setOAuthSessionTelegramInfo(
  sessionId: string,
  chatId: number,
  deviceName: string,
): Promise<void> {
  const kv = await getKv()
  const res = await kv.get<OAuthSession>(["oauth", sessionId])
  if (!res.value) return
  const updated: OAuthSession = {
    ...res.value,
    chat_id: chatId,
    device_name: deviceName,
  }
  await kv.set(["oauth", sessionId], updated, { expireIn: OAUTH_SESSION_TTL })
}
