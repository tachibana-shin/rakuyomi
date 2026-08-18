// urlencode (callable module) -- local replacement for the `urlencode` npm
// package. The package pulled in iconv-lite (and transitively safer-buffer and
// the whole node stream/buffer/events polyfill stack) for non-UTF-8 charsets;
// that was ~46% of the bundle for an API that plugins only use as
// `urlencode(query)`. Behavior matches urlencode@2.0.0 for the UTF-8 path
// byte-for-byte; non-UTF-8 charsets are rejected instead of silently
// producing wrong output.

function isUTF8(charset?: string): boolean {
  if (!charset) return true;
  charset = charset.toLowerCase();
  return charset === "utf8" || charset === "utf-8";
}

export function encode(str: unknown, charset?: string): string {
  if (!isUTF8(charset)) {
    throw new Error(
      `urlencode: charset '${charset}' is not supported by the RakuYomi runner`,
    );
  }
  return encodeURIComponent(String(str));
}

export function decode(str: string, charset?: string): string {
  if (!isUTF8(charset)) {
    throw new Error(
      `urlencode: charset '${charset}' is not supported by the RakuYomi runner`,
    );
  }
  return decodeURIComponent(str);
}

function has(obj: Record<string, unknown>, prop: string): boolean {
  return Object.prototype.hasOwnProperty.call(obj, prop);
}

export function parse(
  qs: string,
  sepOrOptions?: string | { maxKeys?: number; charset?: string },
  eq?: string,
  options?: { maxKeys?: number; charset?: string },
): Record<string, unknown> {
  let sep: string | undefined;
  if (typeof sepOrOptions === "object") {
    options = sepOrOptions;
  } else {
    sep = sepOrOptions;
  }
  sep = sep || "&";
  eq = eq || "=";
  const obj: Record<string, unknown> = {};
  if (typeof qs !== "string" || qs.length === 0) {
    return obj;
  }
  const splits = qs.split(sep);
  let maxKeys = 1000;
  let charset = "";
  if (options) {
    if (typeof options.maxKeys === "number") {
      maxKeys = options.maxKeys;
    }
    if (typeof options.charset === "string") {
      charset = options.charset;
    }
  }
  let len = splits.length;
  if (maxKeys > 0 && len > maxKeys) {
    len = maxKeys;
  }
  for (let i = 0; i < len; ++i) {
    const x = splits[i].replace(/\+/g, "%20");
    const idx = x.indexOf(eq);
    let keyString: string;
    let valueString: string;
    let k: string;
    let v: string;
    if (idx >= 0) {
      keyString = x.substring(0, idx);
      valueString = x.substring(idx + 1);
    } else {
      keyString = x;
      valueString = "";
    }
    if (keyString && keyString.includes("%")) {
      try {
        k = decode(keyString, charset);
      } catch (e) {
        k = keyString;
      }
    } else {
      k = keyString;
    }
    if (valueString && valueString.includes("%")) {
      try {
        v = decode(valueString, charset);
      } catch (e) {
        v = valueString;
      }
    } else {
      v = valueString;
    }
    if (!has(obj, k)) {
      obj[k] = v;
    } else if (Array.isArray(obj[k])) {
      (obj[k] as Array<string>).push(v);
    } else {
      obj[k] = [obj[k], v];
    }
  }
  return obj;
}

function encodeComponent(item: unknown, charset?: string): string {
  const str = String(item);
  if (/^[\x00-\x7F]*$/.test(str)) {
    return encodeURIComponent(str);
  }
  return encode(str, charset);
}

function stringifyArray(
  values: Array<unknown>,
  prefix: string,
  options: { charset?: string },
): string {
  const items: string[] = [];
  for (const [index, value] of values.entries()) {
    items.push(stringify(value, `${prefix}[${index}]`, options));
  }
  return items.join("&");
}

function stringifyObject(
  obj: Record<string, unknown>,
  prefix: string,
  options: { charset?: string },
): string {
  const items: string[] = [];
  const charset = options.charset;
  for (const key in obj) {
    if (key === "") {
      continue;
    }
    const value = obj[key];
    if (value === null || value === undefined) {
      items.push(encode(key, charset) + "=");
    } else {
      const keyPrefix = prefix
        ? prefix + "[" + encodeComponent(key, charset) + "]"
        : encodeComponent(key, charset);
      items.push(stringify(value, keyPrefix, options));
    }
  }
  return items.join("&");
}

export function stringify(
  obj: unknown,
  prefixOrOptions?: string | { charset?: string },
  options?: { charset?: string },
): string {
  let prefix: string | undefined;
  if (typeof prefixOrOptions !== "string") {
    options = prefixOrOptions || {};
  } else {
    prefix = prefixOrOptions;
  }
  options = options ?? {};
  if (Array.isArray(obj)) {
    if (!prefix) {
      throw new TypeError("stringify expects an object");
    }
    return stringifyArray(obj, prefix, options);
  }
  const objValue = String(obj);
  if (obj && typeof obj === "object" && objValue === "[object Object]") {
    return stringifyObject(obj as Record<string, unknown>, prefix ?? "", options);
  }
  if (!prefix) {
    throw new TypeError("stringify expects an object");
  }
  const charset = options?.charset ?? "utf-8";
  return `${prefix}=${encodeComponent(objValue, charset)}`;
}

// The npm package exports the `encode` function itself as the module default,
// with `encode`/`decode`/`parse`/`stringify` as properties; plugins call both
// `urlencode(query)` and occasionally `urlencode.decode(...)`.
const urlencode = Object.assign(
  function urlencode(str: unknown, charset?: string): string {
    return encode(str, charset);
  },
  { encode, decode, parse, stringify },
);

export default urlencode;