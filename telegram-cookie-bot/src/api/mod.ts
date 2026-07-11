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
import webappData from "./routes/api/webapp/data.ts"
import webappCookies from "./routes/api/webapp/cookies.ts"
import webappClear from "./routes/api/webapp/clear.ts"
import webappUnlink from "./routes/api/webapp/unlink.ts"
import webapp from "./routes/webapp.tsx"

const app = new OpenAPIHono()
app.use(etag())
app.use(logger())

app.route("/", health)
app.route("/", pairingGenerate)
app.route("/", pairingStatus)
app.route("/", cookieGet)
app.route("/", cookieSyncAll)
app.route("/", cookieNotifyNeedsUpdate)
app.route("/", cookieDevices)
app.route("/", webappData)
app.route("/", webappCookies)
app.route("/", webappClear)
app.route("/", webappUnlink)
app.route("/", webapp)

app.doc("/doc", {
  openapi: "3.0.0",
  info: {
    title: "Rakuyomi Cookie Sync API",
    version: "1.0.0",
    description: "Cookie sync endpoints for Rakuyomi KOReader plugin",
  },
})

export const apiApp = app
