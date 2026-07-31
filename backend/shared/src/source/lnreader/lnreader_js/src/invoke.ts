// Plugin loading and method dispatch. The plugin code is provided by the Rust
// side as a string (`RAKUYOMI_PLUGIN_CODE`); it is compiled lazily with
// `new Function('require', 'module', ...)`, mirroring how the app evaluates
// plugin bundles. Every method resolves to a JSON string.

import { __require } from "./registry";
import { isUrlAbsolute } from "./webapi";

interface Plugin {
  id?: string;
  name?: string;
  site?: string;
  version?: string;
  icon?: string;
  imageRequestInit?: unknown;
  filters?: Record<string, PluginFilter>;
  pluginSettings?: Record<string, PluginSetting>;
  webStorageUtilized?: boolean;
  popularNovels?: (
    page: number,
    options: { showLatestNovels: boolean; filters: Record<string, unknown> },
  ) => unknown;
  searchNovels?: (query: string, page: number) => unknown;
  parseNovel?: (path: string) => unknown;
  parsePage?: (path: string, chapter: string) => unknown;
  parseChapter?: (path: string) => unknown;
  resolveUrl?: (path: string, isNovel: boolean) => string;
}

interface PluginFilter {
  type?: string;
  value?: unknown;
  options?: Array<{ value: unknown; label: string }>;
}

interface PluginSetting {
  value?: unknown;
}

let plugin: Plugin | null = null;

export function __rakuyomiLoadPlugin(): boolean {
  const code = RAKUYOMI_PLUGIN_CODE;
  const module = { exports: {} };
  const body = "var exports = module.exports = {};\n" + code + ";\nreturn exports.default;";
  const fn = new Function("require", "module", body) as (
    require: (name: string) => unknown,
    module: { exports: Record<string, unknown> },
  ) => Plugin;
  const def = fn(__require as (name: string) => unknown, module);
  if (!def) throw new Error("plugin does not export a default object");
  plugin = def;
  return true;
}

function promiseJson(p: unknown, norm: (v: unknown) => unknown): Promise<string> {
  return Promise.resolve(p).then((v) => {
    return JSON.stringify(norm(v));
  });
}

function normSearch(items: unknown): Array<{ name: string; path: string; cover: string | null }> {
  return ((items as Array<{ name?: unknown; path?: unknown; cover?: unknown }>) || []).map((it) => {
    return {
      name: String(it.name == null ? "" : it.name),
      path: String(it.path == null ? "" : it.path),
      cover: it.cover ? String(it.cover) : null,
    };
  });
}

function parseDate(s: unknown): string | null {
  if (s == null) return null;
  if (typeof s === "number") {
    const d = new Date(s < 100000000000 ? s * 1000 : s);
    return isNaN(d.getTime()) ? null : d.toISOString();
  }
  const str = String(s);
  if (!str) return null;
  const rel = /^(\d+)\s+(minute|hour|day|week|month|year)s?\s+ago$/i.exec(str);
  if (rel) {
    const n = Number(rel[1]);
    const unit = rel[2].toLowerCase();
    const factor: Record<string, number> = {
      minute: 60000,
      hour: 3600000,
      day: 86400000,
      week: 604800000,
      month: 2629746000,
      year: 31556952000,
    };
    return new Date(Date.now() - n * (factor[unit] || 0)).toISOString();
  }
  const dayjsMod = __require("dayjs") as (
    s: string,
    fmt: string,
  ) => { isValid: () => boolean; toISOString: () => string };
  const fmts = [
    "YYYY-MM-DD HH:mm:ss",
    "YYYY-MM-DD HH:mm",
    "YYYY-MM-DDTHH:mm:ssZ",
    "YYYY-MM-DD",
    "MMM D, YYYY",
    "MMMM D, YYYY",
    "MMM D YYYY",
    "MMMM D YYYY",
    "D MMM YYYY",
    "D MMMM YYYY",
    "MM/DD/YYYY",
    "DD/MM/YYYY",
    "MM-DD-YYYY",
    "DD-MM-YYYY",
    "YYYY/MM/DD",
    "hh:mm A",
    "h:mm A",
  ];
  for (const fmt of fmts) {
    const parsed = dayjsMod(str, fmt);
    if (parsed.isValid()) return parsed.toISOString();
  }
  const iso = new Date(str);
  if (!isNaN(iso.getTime())) return iso.toISOString();
  return null;
}

function normChapters(chapters: unknown): Array<Record<string, unknown>> {
  return (
    (chapters as Array<{
      name?: unknown;
      path?: unknown;
      chapterNumber?: unknown;
      releaseTime?: unknown;
      scanlator?: unknown;
      page?: unknown;
    }>) || []
  ).map((c) => {
    const out: Record<string, unknown> = {
      name: String(c.name == null ? "" : c.name),
      path: String(c.path == null ? "" : c.path),
    };
    if (c.chapterNumber != null) out.chapterNumber = Number(c.chapterNumber);
    if (c.releaseTime != null) out.releaseTime = parseDate(c.releaseTime);
    if (c.scanlator != null) {
      out.scanlator = Array.isArray(c.scanlator) ? c.scanlator.join(", ") : c.scanlator;
    }
    if (c.page != null) out.page = String(c.page);
    return out;
  });
}

function normNovel(v: unknown): Record<string, unknown> {
  const src = (v as Record<string, unknown> | null | undefined) || {};
  return {
    name: String(src.name == null ? "" : src.name),
    cover: src.cover ? String(src.cover) : null,
    author: src.author ? String(src.author) : null,
    artist: src.artist ? String(src.artist) : null,
    genres: src.genres ? String(src.genres) : null,
    summary: src.summary ? String(src.summary) : null,
    status: src.status ? String(src.status) : null,
    totalPages: src.totalPages ? Number(src.totalPages) : 1,
    chapters: normChapters(src.chapters),
  };
}

function filtersFromSettings(
  settings: Record<string, unknown>,
): Record<string, { value: unknown }> {
  const out: Record<string, { value: unknown }> = {};
  const declared = (plugin && plugin.filters) || {};
  for (const k in declared) {
    const def = declared[k];
    let val = settings[k] !== undefined ? settings[k] : def && def.value;
    if (def && def.type === "Picker" && typeof val !== "number") {
      const sv = String(val);
      const opts = def.options || [];
      let idx = /^\d+$/.test(sv) ? parseInt(sv, 10) : -1;
      if (idx < 0 || idx >= opts.length) {
        idx = 0;
        for (let i = 0; i < opts.length; i++) {
          if (String(opts[i].value) === sv || String(opts[i].label) === sv) {
            idx = i;
            break;
          }
        }
      }
      val = idx;
    }
    out[k] = { value: val };
  }
  return out;
}

function propsJson(p: Plugin): string {
  return JSON.stringify({
    id: String(p.id == null ? "" : p.id),
    name: String(p.name == null ? "" : p.name),
    site: String(p.site == null ? "" : p.site),
    version: String(p.version == null ? "0.0.0" : p.version),
    icon: p.icon ? String(p.icon) : null,
    imageRequestInit: p.imageRequestInit || null,
    filters: p.filters || null,
    pluginSettings: p.pluginSettings || null,
    hasParsePage: typeof p.parsePage === "function",
    hasResolveUrl: typeof p.resolveUrl === "function",
    webStorageUtilized: !!p.webStorageUtilized,
  });
}

// Mirrors the LNReader app's `resolveUrl` (src/services/plugin/fetch.ts).
export function __resolveUrl(path: string, isNovel: boolean): string {
  const p = plugin;
  if (isUrlAbsolute(path)) return path;
  try {
    if (p && p.resolveUrl) return p.resolveUrl(path, isNovel);
  } catch (e) {
    return path;
  }
  return (p && p.site ? p.site : "") + path;
}

export function __rakuyomiInvoke(method: string, argsJson: string): Promise<string> {
  const p = plugin;
  if (!p) return Promise.reject(new Error("plugin not loaded"));
  let args: unknown[];
  try {
    args = JSON.parse(argsJson) as unknown[];
  } catch (e) {
    return Promise.reject(new Error("invalid args JSON: " + argsJson));
  }
  try {
    switch (method) {
      case "props":
        return Promise.resolve(propsJson(p));
      case "search":
        return promiseJson(
          p.searchNovels ? p.searchNovels(String(args[0]), Number(args[1])) : [],
          normSearch,
        );
      case "popular": {
        const options = {
          showLatestNovels: !!args[2],
          filters: filtersFromSettings((args[1] as Record<string, unknown>) || {}),
        };
        return promiseJson(
          p.popularNovels ? p.popularNovels(Number(args[0]), options) : [],
          normSearch,
        );
      }
      case "novel":
        return promiseJson(p.parseNovel ? p.parseNovel(String(args[0])) : {}, normNovel);
      case "page":
        return promiseJson(p.parsePage ? p.parsePage(String(args[0]), String(args[1])) : [], (v) =>
          normChapters(
            v && (v as Record<string, unknown>).chapters
              ? (v as Record<string, unknown>).chapters
              : v,
          ),
        );
      case "chapter":
        return promiseJson(p.parseChapter ? p.parseChapter(String(args[0])) : "", (html) => {
          return String(html == null ? "" : html);
        });
      case "resolveUrl":
        return promiseJson(__resolveUrl(String(args[0]), !!args[1]), (u) => {
          return String(u == null ? "" : u);
        });
      default:
        return Promise.reject(new Error("unknown plugin method: " + method));
    }
  } catch (e) {
    return Promise.reject(e instanceof Error ? e : new Error(String(e)));
  }
}
