// @libs/fetch -- mirrors the LNReader app's fetch helpers. `fetchApi` adds the
// default headers (user agent, accept-encoding, ...) exactly like the app does.

import { b64Decode, b64Encode, toBytes } from "./b64";
import { Blob, FileReader, FormData, Headers, URLSearchParams } from "./webapi";

export const defaultHeaders: Record<string, string> = {
  Connection: "keep-alive",
  Accept: "*/*",
  "Accept-Language": "*",
  "Sec-Fetch-Mode": "cors",
  "Accept-Encoding": "gzip, deflate",
  "Cache-Control": "max-age=0",
  "User-Agent": RAKUYOMI_USER_AGENT,
};

export function makeInit(init?: Record<string, unknown>): Record<string, unknown> {
  init = init || {};
  let headers: Record<string, string> = {};
  if (init.headers instanceof Headers) {
    for (const k in init.headers._map) headers[k] = init.headers._map[k];
  } else if (init.headers) {
    for (const k in init.headers as Record<string, unknown>) {
      headers[k] = String((init.headers as Record<string, unknown>)[k]);
    }
  }
  const out: Record<string, unknown> = {};
  for (const k in init) {
    if (k !== "headers") out[k] = init[k];
  }
  out.headers = {};
  const outHeaders = out.headers as Record<string, string>;
  for (const dk in defaultHeaders) outHeaders[dk] = defaultHeaders[dk];
  for (const hk in headers) outHeaders[hk] = headers[hk];
  return out;
}

interface SerializedInit {
  method: string;
  headers: Record<string, string>;
  body: string | null;
  bodyB64?: string;
  formData: unknown[] | null;
  timeout: number | null;
}

function serializeInit(init?: Record<string, unknown>): SerializedInit {
  const out: SerializedInit = {
    method: "GET",
    headers: {},
    body: null,
    formData: null,
    timeout: null,
  };
  if (!init) return out;
  if (init.method) out.method = String(init.method).toUpperCase();
  if (init.headers) {
    const h =
      init.headers instanceof Headers
        ? init.headers._map
        : (init.headers as Record<string, unknown>);
    for (const k in h) out.headers[String(k).toLowerCase()] = String(h[k]);
  }
  if (init.timeout) out.timeout = Number(init.timeout);
  if (init.body != null && init.body !== "") {
    const body = init.body as unknown;
    if (body instanceof FormData) {
      out.formData = body._entries.map((e) => ({
        name: e.name,
        value: e.value instanceof Blob ? b64Encode(e.value._bytes) : String(e.value),
        isBlob: e.value instanceof Blob,
        type: e.value instanceof Blob ? e.value._type : "",
        filename: e.filename,
      }));
    } else if (typeof body === "string") {
      out.body = body;
    } else if (body instanceof URLSearchParams) {
      out.body = body.toString();
    } else if (body instanceof Uint8Array || body instanceof ArrayBuffer) {
      out.bodyB64 = b64Encode(toBytes(body));
    } else if (body instanceof Blob) {
      out.bodyB64 = b64Encode(body._bytes);
    } else {
      out.body = String(body);
    }
  }
  return out;
}

export class Response {
  status: number;
  statusText: string;
  ok: boolean;
  url: string;
  private headers_ = new Headers();
  private bodyB64 = "";

  constructor(resp: Record<string, unknown>) {
    this.status = Number(resp.status) || 0;
    this.statusText = String(resp.statusText ?? "");
    this.ok = !!resp.ok;
    this.url = String(resp.url ?? "");
    this.headers_ = new Headers((resp.headers as Record<string, unknown> | undefined) || {});
    this.bodyB64 = String(resp.bodyB64 ?? "");
  }

  headers(): Headers {
    return this.headers_;
  }

  async text(): Promise<string> {
    return __rakuyomiDecode(this.bodyB64, "utf-8");
  }

  async json(): Promise<unknown> {
    return JSON.parse(__rakuyomiDecode(this.bodyB64, "utf-8"));
  }

  async blob(): Promise<Blob> {
    return new Blob([b64Decode(this.bodyB64)], {
      type: this.headers_.get("content-type") || "",
    });
  }
}

export async function fetch(url: string, init?: Record<string, unknown>): Promise<Response> {
  try {
    const respJson = __rakuyomiFetch(String(url), JSON.stringify(serializeInit(init || {})));
    const resp = JSON.parse(respJson) as { error?: boolean; message?: string } & Record<
      string,
      unknown
    >;
    if (resp.error) throw new Error(resp.message || "fetch failed");
    return new Response(resp);
  } catch (e) {
    if (e instanceof Error) throw e;
    throw new Error(String(e));
  }
}

export async function fetchApi(url: string, init?: Record<string, unknown>): Promise<Response> {
  return fetch(url, makeInit(init));
}

export async function fetchText(
  url: string,
  init?: Record<string, unknown>,
  encoding?: string,
): Promise<string> {
  init = makeInit(init);
  try {
    const res = await fetch(url, init);
    if (!res.ok) throw new Error("not ok");
    const blob = await res.blob();
    return await new Promise((resolve, reject) => {
      const fr = new FileReader();
      fr.onloadend = () => resolve(fr.result as string);
      fr.onerror = () => reject(new Error("failed to read blob"));
      fr.onabort = () => reject(new Error("read aborted"));
      fr.readAsText(blob, encoding);
    });
  } catch (e) {
    return "";
  }
}

export function fetchProto(): never {
  throw new Error("fetchProto is not supported by the RakuYomi LNReader runner");
}
