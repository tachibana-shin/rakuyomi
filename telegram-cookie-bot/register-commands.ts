/**
 * Registers the bot's command list with Telegram.
 *
 * This used to run from `src/worker.ts` on the first request of every isolate.
 * It is five `setMyCommands` calls - one per language - with a one second pause
 * between them, so every cold isolate spent several seconds doing it, which is
 * what made the first message after a deploy feel slow. Commands only change
 * when the code changes, so it belongs here instead.
 *
 * Usage:
 *
 *     bun run commands        # uses .env from the working directory
 */
import { createBot } from "./src/bot/mod.ts"
import { registerBotCommands } from "./src/bot/mod.ts"
import { useEnv, type Env } from "./src/env.ts"

// Bun loads `.env` from the working directory into `process.env`, which is
// exactly what the Env shape expects.
useEnv(process.env as unknown as Env)

const bot = createBot()
await registerBotCommands(bot)
console.log("Bot commands registered for every supported language.")
