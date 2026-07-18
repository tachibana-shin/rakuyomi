import { OpenAPIHono } from "@hono/zod-openapi"
import { etag } from "hono/etag"
import { logger } from "hono/logger"

import health from "./routes/health.ts"
import pairingGenerate from "./routes/api/pairing/generate.ts"
import pairingStatus from "./routes/api/pairing/status.ts"
import cookieGet from "./routes/api/cookie/get.ts"
import cookieSyncAll from "./routes/api/cookie/sync-all.ts"
import cookieNotifyNeedsUpdate from "./routes/api/cookie/notify-needs-update.ts"
import cookieDevices from "./routes/api/cookie/devices.ts"
import oauthSession from "./routes/api/oauth/session.ts"
import oauthStatus from "./routes/api/oauth/status/[sessionId].ts"
import oauthCallbacks from "./routes/api/oauth/callbacks.tsx"
import oauthPage from "../page.tsx"
import webappData from "./routes/api/webapp/data.ts"
import webappCookies from "./routes/api/webapp/cookies.ts"
import webappClear from "./routes/api/webapp/clear.ts"
import webappUnlink from "./routes/api/webapp/unlink.ts"
import webapp from "./routes/webapp.tsx"
import { requireApiToken } from "./middleware/auth.ts"

const app = new OpenAPIHono()
app.use(etag())
app.use(logger())

// Public routes
app.route("/", health)
app.route("/", pairingGenerate)
app.route("/", pairingStatus)

// Public webapp routes
app.route("/", webappData)
app.route("/", webappCookies)
app.route("/", webappClear)
app.route("/", webappUnlink)
app.route("/", webapp)

// Protected cookie API routes
app.use("/api/cookie/*", requireApiToken)
app.route("/", cookieGet)
app.route("/", cookieSyncAll)
app.route("/", cookieNotifyNeedsUpdate)
app.route("/", cookieDevices)

// OAuth API routes (no auth — identified by session_id)
app.route("/", oauthSession)
app.route("/", oauthStatus)

// Public OAuth callback and bridge page routes
app.route("/", oauthCallbacks)
app.route("/", oauthPage)

// OpenAPI documentation
app.doc("/doc", {
  openapi: "3.0.0",
  info: {
    title: "RakuYomi Cookie Sync API",
    version: "1.0.0",
    description: "API for RakuYomi KOReader plugin — cookie sync, pairing, and OAuth tracking bridge",
  },
})

export const apiApp = app
