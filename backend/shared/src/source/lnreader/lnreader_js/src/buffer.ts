// Minimal Node-style Buffer used by the bundled packages and some plugins.
// Index access goes through a Proxy, mirroring the previous plain-JS shim.

import { bytesToStr, strToBytes, toBytes } from "./b64";

export interface BufferInstance {
  _bytes: Uint8Array;
  length: number;
  toString(encoding?: string): string;
  slice(start?: number, end?: number): BufferInstance;
  subarray(start?: number, end?: number): BufferInstance;
  equals(other: unknown): boolean;
  write(str: string, offset?: number, length?: number, encoding?: string): number;
  [index: number]: number;
}

function makeBuffer(bytes: Uint8Array): BufferInstance {
  const target: { _bytes: Uint8Array; length: number } = {
    _bytes: bytes,
    length: bytes.length,
  };
  const proxy = new Proxy(target, {
    get(t, prop) {
      if (typeof prop === "string" && /^(0|[1-9]\d*)$/.test(prop)) {
        return Number(prop) < bytes.length ? bytes[Number(prop)] : undefined;
      }
      switch (prop) {
        case "toString":
          return (enc: string) => bytesToStr(bytes, enc);
        case "slice":
          return (start?: number, end?: number) => makeBuffer(bytes.slice(start, end));
        case "subarray":
          return (start?: number, end?: number) => makeBuffer(bytes.slice(start, end));
        case "equals": {
          return (other: unknown) => {
            const o = other instanceof Buffer ? (other as BufferInstance)._bytes : toBytes(other);
            if (o.length !== bytes.length) return false;
            for (let i = 0; i < bytes.length; i++) {
              if (bytes[i] !== o[i]) return false;
            }
            return true;
          };
        }
        case "write": {
          return (str: string, offset: number, _length: number, encoding: string) => {
            const src = strToBytes(str, encoding);
            const start = Number(offset) || 0;
            for (let i = 0; i < src.length && start + i < bytes.length; i++) {
              bytes[start + i] = src[i];
            }
            return src.length;
          };
        }
        default:
          return t[prop as keyof typeof target];
      }
    },
    set(t, prop, value) {
      if (typeof prop === "string" && /^(0|[1-9]\d*)$/.test(prop)) {
        bytes[Number(prop)] = Number(value) & 255;
        return true;
      }
      (t as Record<string, unknown>)[String(prop)] = value;
      return true;
    },
  });
  Object.setPrototypeOf(proxy, Buffer.prototype);
  return proxy as BufferInstance;
}

export function Buffer(arg: unknown, encoding?: string): BufferInstance {
  let bytes: Uint8Array;
  if (typeof arg === "number") {
    bytes = new Uint8Array(Math.max(0, Math.floor(arg)));
  } else if (typeof arg === "string") {
    bytes = strToBytes(arg, encoding);
  } else if (arg instanceof Buffer) {
    bytes = new Uint8Array((arg as BufferInstance)._bytes);
  } else {
    bytes = toBytes(arg);
  }
  return makeBuffer(bytes);
}

Buffer.from = function (arg: unknown, encoding?: string): BufferInstance {
  return Buffer(arg, encoding);
};
Buffer.alloc = function (size: number): BufferInstance {
  return Buffer(size);
};
Buffer.allocUnsafe = function (size: number): BufferInstance {
  return Buffer(size);
};
Buffer.concat = function (list: BufferInstance[]): BufferInstance {
  let total = 0;
  for (const b of list) total += b._bytes.length;
  const bytes = new Uint8Array(total);
  let offset = 0;
  for (const b of list) {
    bytes.set(b._bytes, offset);
    offset += b._bytes.length;
  }
  return makeBuffer(bytes);
};
Buffer.isBuffer = function (v: unknown): boolean {
  return v instanceof Buffer;
};

export function SlowBuffer(size: number): BufferInstance {
  return Buffer(size);
}
