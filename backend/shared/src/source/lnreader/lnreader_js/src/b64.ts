// Byte/base64/hex helpers and TextEncoder/TextDecoder. Encoding is delegated
// to the Rust host functions so the plugin runtime stays dependency-free.

const B64_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

export function b64Encode(bytes: Uint8Array): string {
  let out = "";
  let i = 0;
  for (; i + 2 < bytes.length; i += 3) {
    const n = (bytes[i] << 16) | (bytes[i + 1] << 8) | bytes[i + 2];
    out +=
      B64_ALPHABET[(n >> 18) & 63] +
      B64_ALPHABET[(n >> 12) & 63] +
      B64_ALPHABET[(n >> 6) & 63] +
      B64_ALPHABET[n & 63];
  }
  const rem = bytes.length - i;
  if (rem === 1) {
    const n1 = bytes[i] << 16;
    out += B64_ALPHABET[(n1 >> 18) & 63] + B64_ALPHABET[(n1 >> 12) & 63] + "==";
  } else if (rem === 2) {
    const n2 = (bytes[i] << 16) | (bytes[i + 1] << 8);
    out +=
      B64_ALPHABET[(n2 >> 18) & 63] +
      B64_ALPHABET[(n2 >> 12) & 63] +
      B64_ALPHABET[(n2 >> 6) & 63] +
      "=";
  }
  return out;
}

export function b64Decode(str: string): Uint8Array {
  str = String(str).replace(/[^A-Za-z0-9+/=]/g, "");
  const len = str.length;
  const bytes = new Uint8Array(Math.floor((len * 3) / 4));
  let p = 0;
  for (let i = 0; i < len; i += 4) {
    const a = B64_ALPHABET.indexOf(str[i]);
    const b = B64_ALPHABET.indexOf(str[i + 1] || "A");
    const c = B64_ALPHABET.indexOf(str[i + 2] || "A");
    const d = B64_ALPHABET.indexOf(str[i + 3] || "A");
    const n = (a << 18) | (b << 12) | ((c === -1 ? 0 : c) << 6) | (d === -1 ? 0 : d);
    bytes[p++] = (n >> 16) & 255;
    if (str[i + 2] && str[i + 2] !== "=") bytes[p++] = (n >> 8) & 255;
    if (str[i + 3] && str[i + 3] !== "=") bytes[p++] = n & 255;
  }
  return bytes.subarray(0, p);
}

export function bytesToHex(bytes: Uint8Array): string {
  let out = "";
  for (let i = 0; i < bytes.length; i++) {
    const h = bytes[i].toString(16);
    out += h.length === 1 ? "0" + h : h;
  }
  return out;
}

export function hexToBytes(hex: string): Uint8Array {
  hex = String(hex).replace(/\s/g, "");
  if (hex.length % 2 !== 0) hex = "0" + hex;
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = parseInt(hex.substr(i * 2, 2), 16);
  }
  return bytes;
}

export function toBytes(v: unknown): Uint8Array {
  if (v instanceof Uint8Array) return v;
  if (v instanceof ArrayBuffer) return new Uint8Array(v);
  if (Array.isArray(v)) return new Uint8Array(v);
  if (v && typeof v === "object" && (v as { buffer?: unknown }).buffer instanceof ArrayBuffer) {
    const o = v as {
      buffer: ArrayBuffer;
      byteOffset?: number;
      byteLength?: number;
      length?: number;
    };
    return new Uint8Array(o.buffer, o.byteOffset || 0, o.byteLength ?? o.length ?? 0);
  }
  return new Uint8Array(0);
}

export function strToBytes(str: string, encoding?: string): Uint8Array {
  str = String(str);
  encoding = (encoding || "utf8").toLowerCase();
  if (encoding === "utf8" || encoding === "utf-8" || encoding === "unicode-1-1-utf-8") {
    return b64Decode(__rakuyomiEncodeUtf8(str));
  }
  if (encoding === "base64") return b64Decode(str);
  if (encoding === "hex") return hexToBytes(str);
  // latin1 / binary / ascii: byte per char
  const bytes = new Uint8Array(str.length);
  for (let i = 0; i < str.length; i++) bytes[i] = str.charCodeAt(i) & 255;
  return bytes;
}

export function bytesToStr(bytes: Uint8Array, encoding?: string): string {
  encoding = (encoding || "utf8").toLowerCase();
  if (encoding === "utf8" || encoding === "utf-8" || encoding === "unicode-1-1-utf-8") {
    return __rakuyomiDecode(b64Encode(bytes), "utf-8");
  }
  if (encoding === "base64") return b64Encode(bytes);
  if (encoding === "hex") return bytesToHex(bytes);
  let out = "";
  for (let i = 0; i < bytes.length; i++) out += String.fromCharCode(bytes[i]);
  return out;
}

export function utf8ToBytes(str: string): Uint8Array {
  return b64Decode(__rakuyomiEncodeUtf8(String(str)));
}

export function bytesToUtf8(bytes: Uint8Array): string {
  return __rakuyomiDecode(b64Encode(toBytes(bytes)), "utf-8");
}

export class TextEncoder {
  encode(str: string | null | undefined): Uint8Array {
    return b64Decode(__rakuyomiEncodeUtf8(String(str == null ? "" : str)));
  }
}

export class TextDecoder {
  private encoding: string;

  constructor(encoding?: string) {
    this.encoding = encoding || "utf-8";
  }

  decode(bytes: unknown): string {
    return __rakuyomiDecode(b64Encode(toBytes(bytes)), this.encoding);
  }
}
