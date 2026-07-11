export function getConfig() {
  const BOT_TOKEN = Deno.env.get("BOT_TOKEN")
  if (!BOT_TOKEN) {
    throw new Error("BOT_TOKEN environment variable is required")
  }

  const USE_WEBHOOK = Deno.env.get("USE_WEBHOOK") === "true"
  const WEBHOOK_URL = Deno.env.get("WEBHOOK_URL") ?? ""
  const PUBLIC_URL = Deno.env.get("PUBLIC_URL") ?? ""

  const TURSO_DB_URL = Deno.env.get("TURSO_DB_URL") ?? ""
  const TURSO_AUTH_TOKEN = Deno.env.get("TURSO_AUTH_TOKEN") ?? ""

  return { BOT_TOKEN, USE_WEBHOOK, WEBHOOK_URL, PUBLIC_URL, TURSO_DB_URL, TURSO_AUTH_TOKEN }
}
