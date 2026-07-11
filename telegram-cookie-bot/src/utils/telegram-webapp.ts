const encoder = new TextEncoder()

function bytesToHex(bytes: ArrayBuffer): string {
  return Array.from(new Uint8Array(bytes))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("")
}

export async function verifyTelegramWebAppData(
  initData: string,
  botToken: string,
): Promise<{ userId: number } | null> {
  const params = new URLSearchParams(initData)
  const hash = params.get("hash")
  if (!hash) return null

  params.delete("hash")

  const sorted: [string, string][] = []
  for (const [key, value] of params) {
    sorted.push([key, value])
  }
  sorted.sort((a, b) => a[0].localeCompare(b[0]))

  const dataCheckString = sorted
    .map(([k, v]) => `${k}=${v}`)
    .join("\n")

  const webAppDataKey = await crypto.subtle.importKey(
    "raw",
    encoder.encode("WebAppData"),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  )
  const secretKeyBytes = await crypto.subtle.sign(
    "HMAC",
    webAppDataKey,
    encoder.encode(botToken),
  )

  const signKey = await crypto.subtle.importKey(
    "raw",
    secretKeyBytes,
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  )
  const computedHash = bytesToHex(
    await crypto.subtle.sign(
      "HMAC",
      signKey,
      encoder.encode(dataCheckString),
    ),
  )

  if (computedHash !== hash) return null

  const userStr = params.get("user")
  if (!userStr) return null

  try {
    const user = JSON.parse(userStr)
    if (user?.id) {
      return { userId: user.id }
    }
  } catch {
    return null
  }
  return null
}
