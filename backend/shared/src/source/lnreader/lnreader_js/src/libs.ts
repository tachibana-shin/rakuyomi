// Entry point of the LNReader plugin runtime.
//
// Built with bun into a single IIFE (`assets/libs.js`) and evaluated once per
// source worker in global scope, after the Rust side has injected the globals
// declared in `globals.d.ts`. Because the bundler wraps this file in a
// function scope, everything plugins may reach as a bare global is attached
// to `globalThis` here explicitly.

import { TextDecoder, TextEncoder } from "./b64";
import { atob, btoa, Buffer, SlowBuffer } from "./buffer";
import {
  AbortController,
  Blob,
  clearInterval,
  clearTimeout,
  consoleObj,
  FileReader,
  FormData,
  Headers,
  intl,
  setInterval,
  setTimeout,
  URL,
  URLSearchParams,
} from "./webapi";
import { fetch, Response } from "./fetch";
import { FilterTypes, NovelStatus, registerModules } from "./registry";
import { __rakuyomiInvoke, __rakuyomiLoadPlugin, __resolveUrl } from "./invoke";

registerModules();

const g = globalThis as Record<string, unknown>;
g.TextEncoder = TextEncoder;
g.TextDecoder = TextDecoder;
g.Buffer = Buffer;
g.SlowBuffer = SlowBuffer;
g.atob = atob;
g.btoa = btoa;
g.Headers = Headers;
g.FormData = FormData;
g.Blob = Blob;
g.FileReader = FileReader;
g.URLSearchParams = URLSearchParams;
g.URL = URL;
g.Intl = intl;
g.setTimeout = setTimeout;
g.clearTimeout = clearTimeout;
g.setInterval = setInterval;
g.clearInterval = clearInterval;
g.AbortController = AbortController;
g.console = consoleObj;
g.fetch = fetch;
g.Response = Response;
g.__rakuyomiInvoke = __rakuyomiInvoke;
g.__rakuyomiLoadPlugin = __rakuyomiLoadPlugin;
g.__resolveUrl = __resolveUrl;
g.FilterTypes = FilterTypes;
g.NovelStatus = NovelStatus;
