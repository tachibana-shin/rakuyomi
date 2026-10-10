/**
 * Test environment.
 *
 * Tests run outside Cloudflare, so no bindings exist. Publishing an empty
 * environment makes `getConfig()` return empty strings - the Turso client then
 * resolves to `null` - and makes the key/value store fall back to its
 * in-memory backend. Tests are therefore hermetic and never touch the database.
 */
import { useEnv, type Env } from "../src/env.ts"

/** Publishes an empty environment for the current test process. */
export function useTestEnv(): void {
  useEnv({} as Env)
}
