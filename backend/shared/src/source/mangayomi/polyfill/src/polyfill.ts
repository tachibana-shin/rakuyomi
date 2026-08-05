// Entry point of the MangaYomi JavaScript extension polyfill.
//
// Ported from the mangayomi app's `eval/javascript/*.dart` bridge classes so
// extensions behave exactly like they do on the app. Built with bun into a
// single IIFE (`js_assets/polyfill.js`) and evaluated once per extension
// worker in global scope, after the Rust side has injected the globals
// declared in `globals.d.ts` (`sendMessage`, `RAKUYOMI_SOURCE`). Because the
// bundler wraps this file in a function scope, everything extensions may
// reach as a bare global is attached to `globalThis` here explicitly.

import { Client } from "./client";
import { consoleObj } from "./console";
import { Document, Element } from "./dom";
import { helperApi } from "./helpers";
import { SharedPreferences } from "./preferences";
import { jsonStringify, MProvider } from "./provider";
import { installStringHelpers } from "./strings";

installStringHelpers();

const g = globalThis as Record<string, unknown>;

if (g.console === undefined) {
    g.console = consoleObj;
}
for (const [name, fn] of Object.entries(helperApi)) {
    g[name] = fn;
}
g.Client = Client;
g.SharedPreferences = SharedPreferences;
g.Document = Document;
g.Element = Element;
g.MProvider = MProvider;
g.jsonStringify = jsonStringify;
