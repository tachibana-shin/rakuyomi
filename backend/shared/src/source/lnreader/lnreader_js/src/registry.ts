// Plugin-facing module registry. Bundled npm packages are inlined by bun into
// the single output file; `__require` hands them out under the same names the
// LNReader app uses (`cheerio`, `dayjs`, `@libs/*`, ...).

import * as cheerio from "cheerio";
import dayjs from "dayjs";
import * as htmlparser2 from "htmlparser2";
import urlencode from "urlencode";
import { gcm } from "@noble/ciphers/aes.js";

import { Buffer, SlowBuffer } from "./buffer";
import { bytesToUtf8, utf8ToBytes } from "./b64";
import { isUrlAbsolute } from "./webapi";
import { storageApi } from "./storage";
import { fetchApi, fetchProto, fetchText } from "./fetch";

const modules: Record<string, unknown> = {};

export function __require(name: string): unknown {
  if (name in modules) return modules[name];
  throw new Error("Module not found: " + name);
}

export function __register(name: string, module: unknown): void {
  modules[name] = module;
}

export const FilterTypes = {
  TextInput: "Text",
  Picker: "Picker",
  CheckboxGroup: "Checkbox",
  Switch: "Switch",
  ExcludableCheckboxGroup: "XCheckbox",
};

export const NovelStatus = {
  Unknown: "Unknown",
  Ongoing: "Ongoing",
  Completed: "Completed",
  Licensed: "Licensed",
  PublishingFinished: "Publishing Finished",
  Cancelled: "Cancelled",
  OnHiatus: "On Hiatus",
  STUB: "STUB",
  Inactive: "Inactive",
};

export const defaultCover =
  "https://github.com/lnreader/lnreader-plugins/blob/master/public/static/coverNotAvailable.webp?raw=true";

export function registerModules(): void {
  __register("buffer", {
    Buffer,
    SlowBuffer,
    INSPECT_MAX_BYTES: 50,
  });
  __register("cheerio", cheerio);
  __register("dayjs", dayjs);
  __register("htmlparser2", htmlparser2);
  __register("urlencode", urlencode);

  __register("@libs/novelStatus", { NovelStatus });
  __register("@libs/defaultCover", { defaultCover });
  __register("@libs/isAbsoluteUrl", { isUrlAbsolute });
  __register("@libs/filterInputs", { FilterTypes });
  __register("@libs/aes", {
    gcm: (key: Uint8Array, nonce: Uint8Array) => gcm(key, nonce),
  });
  __register("@libs/utils", { utf8ToBytes, bytesToUtf8 });
  __register("@libs/storage", storageApi);
  __register("@libs/fetch", { fetchApi, fetchText, fetchProto });
}
