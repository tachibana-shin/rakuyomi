import { createMiddleware } from "hono/factory"
import { verifyChatToken } from "../../store.ts"

export const requireApiToken = createMiddleware(async (c, next) => {
  const auth = c.req.header("Authorization")
  if (!auth || !auth.startsWith("Bearer ")) {
    return c.json({ error: "Unauthorized" }, 401)
  }

  const token = auth.slice(7)
  const chatId = c.req.query("chat_id")

  if (!chatId || !(await verifyChatToken(Number(chatId), token))) {
    return c.json({ error: "Unauthorized" }, 401)
  }

  await next()
})
