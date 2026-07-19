import { OpenAPIHono } from "@hono/zod-openapi";
import { Bot, webhookCallback } from "grammy";
import { etag } from "hono/etag";
import { logger } from "hono/logger";

// Routes
import health from "./api/routes/health.ts";
import pairingGenerate from "./api/routes/api/pairing/generate.ts";
import pairingStatus from "./api/routes/api/pairing/status.ts";
import cookieGet from "./api/routes/api/cookie/get.ts";
import cookieSyncAll from "./api/routes/api/cookie/sync-all.ts";
import cookieNotifyNeedsUpdate from "./api/routes/api/cookie/notify-needs-update.ts";
import cookieDevices from "./api/routes/api/cookie/devices.ts";
import oauthSession from "./api/routes/api/oauth/session.ts";
import oauthStatus from "./api/routes/api/oauth/status/[sessionId].ts";
import oauthAnilistCallback from "./routes/oauth/anilist/callback.ts";
import oauthMalCallback from "./routes/oauth/myanimelist/callback.ts";
import oauthShikimoriCallback from "./routes/oauth/shikimori/callback.ts";
import oauthBangumiCallback from "./routes/oauth/bangumi/callback.ts";
import oauthMangabakaCallback from "./routes/oauth/mangabaka/callback.ts";
import oauthBridge from "./routes/oauth/[service]/[sessionId].tsx";
import webappData from "./api/routes/api/webapp/data.ts";
import webappCookies from "./api/routes/api/webapp/cookies.ts";
import webappClear from "./api/routes/api/webapp/clear.ts";
import webappUnlink from "./api/routes/api/webapp/unlink.ts";
import webapp from "./api/routes/webapp.tsx";
import { requireApiToken } from "./api/middleware/auth.ts";

const app = new OpenAPIHono();
app.use(etag());
app.use(logger());

// Public routes
app.route("/", health);
app.route("/", pairingGenerate);
app.route("/", pairingStatus);

// Public webapp routes
app.route("/", webappData);
app.route("/", webappCookies);
app.route("/", webappClear);
app.route("/", webappUnlink);
app.route("/", webapp);

// Protected cookie API routes
app.use("/api/cookie/*", requireApiToken);
app.route("/", cookieGet);
app.route("/", cookieSyncAll);
app.route("/", cookieNotifyNeedsUpdate);
app.route("/", cookieDevices);

// OAuth API routes (no auth - identified by session_id)
app.route("/", oauthSession);
app.route("/", oauthStatus);

// OAuth callback and bridge routes (public, browser-facing)
app.route("/", oauthAnilistCallback);
app.route("/", oauthMalCallback);
app.route("/", oauthShikimoriCallback);
app.route("/", oauthBangumiCallback);
app.route("/", oauthMangabakaCallback);
app.route("/", oauthBridge);

// OpenAPI documentation
app.doc("/doc", {
  openapi: "3.0.0",
  info: {
    title: "RakuYomi Cookie Sync API",
    version: "1.0.0",
    description:
      "API for RakuYomi KOReader plugin - cookie sync, pairing, and OAuth tracking bridge",
  },
});

export function startWebhookServer(bot: Bot | null, webhookUrl: string) {
  if (bot !== null) {
    const webhookHandler = webhookCallback(bot, "std/http");

    app.post("/webhook", async (c) => {
      return await webhookHandler(c.req.raw);
    });
  }

  Deno.serve(app.fetch);
  console.log(`Bot running in webhook mode: ${webhookUrl}/webhook`);
}

export function startPollingServer(bot: Bot) {
  bot.start();
  console.log("Bot running in polling mode. Press Ctrl+C to stop.");

  Deno.serve({ port: 8788 }, app.fetch);
}
