// Browser-ish Web APIs needed by plugins: Headers, FormData, Blob,
// FileReader, URLSearchParams, URL, a minimal Intl.DateTimeFormat, timers,
// AbortController and console. All backed by the synchronous host functions.

import { b64Encode, bytesToStr, toBytes } from "./b64";

// ---------------------------------------------------------------------------
// Headers
// ---------------------------------------------------------------------------

export class Headers {
  _map: Record<string, string> = {};

  constructor(init?: Headers | Record<string, unknown> | null) {
    if (init instanceof Headers) {
      for (const k in init._map) this._map[k] = init._map[k];
    } else if (init) {
      for (const k in init) {
        this._map[String(k).toLowerCase()] = String(init[k]);
      }
    }
  }

  get(name: string): string | null {
    return this._map[String(name).toLowerCase()] ?? null;
  }

  has(name: string): boolean {
    return this._map[String(name).toLowerCase()] !== undefined;
  }

  set(name: string, value: string): void {
    this._map[String(name).toLowerCase()] = String(value);
  }

  append(name: string, value: string): void {
    const key = String(name).toLowerCase();
    if (this._map[key] !== undefined) {
      this._map[key] = this._map[key] + ", " + String(value);
    } else {
      this._map[key] = String(value);
    }
  }

  delete(name: string): void {
    delete this._map[String(name).toLowerCase()];
  }

  forEach(cb: (value: string, name: string) => void): void {
    for (const k in this._map) cb(this._map[k], k);
  }

  entries(): [string, string][] {
    const out: [string, string][] = [];
    for (const k in this._map) out.push([k, this._map[k]]);
    return out;
  }

  keys(): string[] {
    return Object.keys(this._map);
  }

  values(): string[] {
    const out: string[] = [];
    for (const k in this._map) out.push(this._map[k]);
    return out;
  }
}

// ---------------------------------------------------------------------------
// FormData
// ---------------------------------------------------------------------------

export interface FormDataEntry {
  name: string;
  value: string | Blob;
  filename?: string;
}

export class FormData {
  _entries: FormDataEntry[] = [];

  append(name: string, value: string | Blob, filename?: string): void {
    this._entries.push({ name: String(name), value, filename });
  }

  set(name: string, value: string | Blob, filename?: string): void {
    this._entries = this._entries.filter((e) => e.name !== String(name));
    this.append(name, value, filename);
  }

  get(name: string): string | Blob | null {
    for (const e of this._entries) {
      if (e.name === String(name)) return e.value;
    }
    return null;
  }

  getAll(name: string): Array<string | Blob> {
    return this._entries.filter((e) => e.name === String(name)).map((e) => e.value);
  }

  has(name: string): boolean {
    return this.get(name) !== null;
  }

  delete(name: string): void {
    this._entries = this._entries.filter((e) => e.name !== String(name));
  }

  entries(): FormDataEntry[] {
    return this._entries.slice();
  }

  keys(): string[] {
    return this._entries.map((e) => e.name);
  }

  values(): Array<string | Blob> {
    return this._entries.map((e) => e.value);
  }

  forEach(cb: (value: string | Blob, name: string) => void): void {
    for (const e of this._entries) cb(e.value, e.name);
  }
}

// ---------------------------------------------------------------------------
// Blob / FileReader
// ---------------------------------------------------------------------------

function concatBytes(parts: unknown[]): Uint8Array {
  const byteParts = parts.map((p) => toBytes(p));
  let total = 0;
  for (const p of byteParts) total += p.length;
  const out = new Uint8Array(total);
  let offset = 0;
  for (const p of byteParts) {
    out.set(p, offset);
    offset += p.length;
  }
  return out;
}

export class Blob {
  _bytes: Uint8Array;
  _type: string;
  size: number;
  type: string;

  constructor(parts?: unknown[], opts?: { type?: string }) {
    this._bytes = concatBytes(parts || []);
    this._type = (opts && opts.type) || "";
    this.size = this._bytes.length;
    this.type = this._type;
  }

  slice(start?: number, end?: number): Blob {
    return new Blob([this._bytes.slice(start, end)], { type: this._type });
  }

  text(): Promise<string> {
    return new Promise((resolve) => {
      resolve(bytesToStr(this._bytes, "utf8"));
    });
  }
}

export class FileReader {
  result: unknown = null;
  onloadend: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onabort: (() => void) | null = null;

  private done(result: unknown): void {
    this.result = result;
    if (this.onloadend) this.onloadend();
  }

  readAsText(blob: Blob, encoding?: string): void {
    try {
      this.done(bytesToStr(blob._bytes, encoding));
    } catch (e) {
      if (this.onerror) this.onerror();
    }
  }

  readAsDataURL(blob: Blob): void {
    try {
      this.done("data:" + blob._type + ";base64," + b64Encode(blob._bytes));
    } catch (e) {
      if (this.onerror) this.onerror();
    }
  }
}

// ---------------------------------------------------------------------------
// URLSearchParams / URL
// ---------------------------------------------------------------------------

export class URLSearchParams {
  private params: [string, string][] = [];

  constructor(init?: string | URLSearchParams | Record<string, unknown>) {
    if (typeof init === "string") {
      if (init) {
        for (const part of init.split("&")) {
          if (!part) continue;
          const eq = part.indexOf("=");
          if (eq === -1) {
            this.params.push([part, ""]);
          } else {
            this.params.push([part.slice(0, eq), part.slice(eq + 1)]);
          }
        }
      }
    } else if (init instanceof URLSearchParams) {
      this.params = init.params.slice();
    } else if (init) {
      for (const k in init) this.params.push([k, String(init[k])]);
    }
  }

  append(name: string, value: string): void {
    this.params.push([String(name), String(value)]);
  }

  set(name: string, value: string): void {
    this.params = this.params.filter((p) => p[0] !== String(name));
    this.append(name, value);
  }

  get(name: string): string | null {
    for (const p of this.params) {
      if (p[0] === String(name)) return p[1];
    }
    return null;
  }

  getAll(name: string): string[] {
    return this.params.filter((p) => p[0] === String(name)).map((p) => p[1]);
  }

  has(name: string): boolean {
    return this.get(name) !== null;
  }

  delete(name: string): void {
    this.params = this.params.filter((p) => p[0] !== String(name));
  }

  toString(): string {
    return this.params.map((p) => p[0] + "=" + p[1]).join("&");
  }

  entries(): Iterator<[string, string]> {
    return this.params.slice()[Symbol.iterator]();
  }

  keys(): Iterator<string> {
    return this.params
      .slice()
      .map((p) => p[0])
      [Symbol.iterator]();
  }

  values(): Iterator<string> {
    return this.params
      .slice()
      .map((p) => p[1])
      [Symbol.iterator]();
  }

  forEach(cb: (value: string, name: string) => void): void {
    for (const p of this.params) cb(p[1], p[0]);
  }
}

function urlNormalizePath(p: string): string {
  const leading = p.charAt(0) === "/" ? "/" : "";
  const parts = p.split("/");
  const stack: string[] = [];
  for (const part of parts) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      if (stack.length > 0) stack.pop();
      continue;
    }
    stack.push(part);
  }
  return leading + stack.join("/");
}

export function isUrlAbsolute(url: string): boolean {
  if (url) {
    if (url.indexOf("//") === 0) return true;
    if (url.indexOf("://") === -1) return false;
    if (url.indexOf(".") === -1) return false;
    if (url.indexOf("/") === -1) return false;
    if (url.indexOf(":") > url.indexOf("/")) return false;
    if (url.indexOf("://") < url.indexOf(".")) return true;
  }
  return false;
}

export function urlResolve(base: string, ref: string): string {
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(ref)) return ref;
  if (ref === "") return base;
  if (ref.charAt(0) === "#") return base.split("#")[0] + ref;
  if (ref.charAt(0) === "?") return base.split("?")[0].split("#")[0] + ref;
  const m = /^([a-zA-Z][a-zA-Z0-9+.-]*:)?(\/\/[^/?#]*)?([^?#]*)(\?[^#]*)?(#.*)?$/.exec(base);
  const protocol = (m && m[1]) || "";
  const authority = (m && m[2]) || "";
  const basePath = (m && m[3]) || "";
  const baseQuery = (m && m[4]) || "";
  if (ref.indexOf("//") === 0) return protocol + ref;
  let path: string;
  if (ref.charAt(0) === "/") {
    path = ref;
  } else {
    const dir = basePath.indexOf("/") >= 0 ? basePath.slice(0, basePath.lastIndexOf("/") + 1) : "/";
    path = urlNormalizePath(dir + ref);
  }
  return protocol + authority + path + baseQuery;
}

export class URL {
  href: string;
  protocol: string;
  hostname: string;
  port: string;
  host: string;
  pathname: string;
  search: string;
  hash: string;
  origin: string;
  searchParams: URLSearchParams;

  constructor(url: string, base?: string) {
    if (!url) throw new TypeError("Invalid URL");
    let target: string;
    if (base) {
      target = urlResolve(String(base), String(url));
    } else {
      target = String(url);
    }
    const m = /^(?:([a-zA-Z][a-zA-Z0-9+.-]*):)?(?:\/\/([^/?#]*))?([^?#]*)(\?[^#]*)?(#.*)?$/.exec(
      target,
    );
    this.href = target;
    this.protocol = m && m[1] ? m[1] + ":" : "";
    let host = (m && m[2]) || "";
    let hostname = host;
    let port = "";
    const hm = /^(.*):(\d+)$/.exec(host);
    if (hm) {
      hostname = hm[1];
      port = hm[2];
    }
    this.hostname = hostname;
    this.port = port;
    this.host = host;
    this.pathname = (m && m[3]) || "/";
    this.search = (m && m[4]) || "";
    this.hash = (m && m[5]) || "";
    this.origin = this.protocol ? this.protocol + "//" + this.host : "null";
    this.searchParams = new URLSearchParams(this.search);
  }

  toString(): string {
    return this.href;
  }

  toJSON(): string {
    return this.href;
  }
}

// ---------------------------------------------------------------------------
// Intl shim (minimal DateTimeFormat)
// ---------------------------------------------------------------------------

const INTL_MONTHS = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];
const INTL_MONTHS_SHORT = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
];
const INTL_DAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const INTL_DAYS_SHORT = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

export class DateTimeFormat {
  private month?: string;
  private day?: string;
  private year?: string;
  private weekday?: string;
  private hour?: string;
  private minute?: string;
  private second?: string;
  private hour12: boolean;

  constructor(_locales: unknown, options?: Record<string, string | boolean>) {
    options = options || {};
    this.month = options.month as string | undefined;
    this.day = options.day as string | undefined;
    this.year = options.year as string | undefined;
    this.weekday = options.weekday as string | undefined;
    this.hour = options.hour as string | undefined;
    this.minute = options.minute as string | undefined;
    this.second = options.second as string | undefined;
    this.hour12 = options.hour12 !== false;
  }

  format(date: Date | number | string): string {
    let d: Date;
    if (date instanceof Date) d = date;
    else if (typeof date === "number") d = new Date(date);
    else d = new Date(String(date));
    if (isNaN(d.getTime())) return String(date);
    const parts: string[] = [];
    if (this.weekday) {
      parts.push(this.weekday === "long" ? INTL_DAYS[d.getDay()] : INTL_DAYS_SHORT[d.getDay()]);
    }
    const dayStr = this.day === "2-digit" ? ("0" + d.getDate()).slice(-2) : String(d.getDate());
    if (this.month === "long") {
      parts.push(INTL_MONTHS[d.getMonth()] + " " + dayStr);
    } else if (this.month === "short") {
      parts.push(INTL_MONTHS_SHORT[d.getMonth()] + " " + dayStr);
    } else {
      parts.push(
        (this.month === "2-digit"
          ? ("0" + (d.getMonth() + 1)).slice(-2)
          : String(d.getMonth() + 1)) +
          "/" +
          dayStr,
      );
    }
    if (this.year !== undefined && this.year !== null) {
      parts.push(this.year === "2-digit" ? String(d.getFullYear() % 100) : String(d.getFullYear()));
    }
    if (this.hour !== undefined || this.minute !== undefined || this.second !== undefined) {
      let h = d.getHours();
      let suffix = "";
      if (this.hour12) {
        suffix = h >= 12 ? " PM" : " AM";
        h = h % 12 || 12;
      }
      const hStr = this.hour === "2-digit" ? ("0" + h).slice(-2) : String(h);
      const mStr = this.minute !== undefined ? ":" + ("0" + d.getMinutes()).slice(-2) : "";
      const sStr = this.second !== undefined ? ":" + ("0" + d.getSeconds()).slice(-2) : "";
      parts.push(hStr + mStr + sStr + suffix);
    }
    return parts.join(", ");
  }

  formatToParts(date: Date | number | string): Array<{ type: string; value: string }> {
    return [{ type: "literal", value: this.format(date) }];
  }
}

export const intl = {
  DateTimeFormat,
};

// ---------------------------------------------------------------------------
// timers (synchronous, blocking the worker -- fine for scripts)
// ---------------------------------------------------------------------------

let timerSeq = 0;

export function setTimeout(
  cb: (...args: unknown[]) => void,
  ms?: number,
  ...args: unknown[]
): number {
  if (typeof cb === "function") {
    const delay = ms && ms > 0 ? ms : 0;
    __rakuyomiSleep(delay);
    cb.apply(null, args);
  }
  return ++timerSeq;
}

export function clearTimeout(): void {}

export function setInterval(): number {
  return 0;
}

export function clearInterval(): void {}

// ---------------------------------------------------------------------------
// AbortController (no-op signal)
// ---------------------------------------------------------------------------

export class AbortController {
  signal: { aborted: boolean } = { aborted: false };

  abort(): void {
    this.signal.aborted = true;
  }
}

// ---------------------------------------------------------------------------
// console (forwarded to the host log)
// ---------------------------------------------------------------------------

function consoleFormat(v: unknown): string {
  if (typeof v === "string") return v;
  if (v instanceof Error) return v.message;
  try {
    return JSON.stringify(v);
  } catch (e) {
    return String(v);
  }
}

export const consoleObj = {
  log: (...args: unknown[]) => __rakuyomiLog("log", args.map(consoleFormat).join(" ")),
  info: (...args: unknown[]) => __rakuyomiLog("log", args.map(consoleFormat).join(" ")),
  warn: (...args: unknown[]) => __rakuyomiLog("warn", args.map(consoleFormat).join(" ")),
  error: (...args: unknown[]) => __rakuyomiLog("error", args.map(consoleFormat).join(" ")),
  debug: (...args: unknown[]) => __rakuyomiLog("log", args.map(consoleFormat).join(" ")),
};
