// Globals injected by the Rust side (backend/shared/src/source/mangayomi/js/mod.rs)
// before the polyfill is evaluated, plus the synchronous host function.
//
// The polyfill is evaluated once per extension worker inside the embedded JS
// engine (quickjs via rquickjs), in global scope.

declare const sendMessage: (name: string, argsJson: string) => string;

declare const RAKUYOMI_SOURCE: MSource;

/// The `MSource` object the `MProvider.source` getter returns (the Rust side
/// serialises the stored `index.json` entry, mirroring `MSource.toJson()`).
interface MSource {
    id?: unknown;
    name?: string;
    lang?: string;
    baseUrl?: string;
    apiUrl?: string;
    dateFormat?: string;
    dateFormatLocale?: string;
    additionalParams?: string;
    notes?: string;
    isFullData?: boolean;
    hasCloudflare?: boolean;
    [key: string]: unknown;
}

/// The polyfill classes as ambient globals, so extension bundles (e.g. the
/// offline test fixture) type-check against the same surface the runtime
/// attaches to `globalThis` at evaluation time.
interface ResponseBody {
    body?: string;
    headers?: Record<string, string>;
    statusCode?: number;
}

declare class Client {
    constructor(reqcopyWith?: unknown);
    head(url: string, headers?: unknown): Promise<ResponseBody>;
    get(url: string, headers?: unknown): Promise<ResponseBody>;
    post(url: string, headers?: unknown, body?: unknown): Promise<ResponseBody>;
    put(url: string, headers?: unknown, body?: unknown): Promise<ResponseBody>;
    delete(url: string, headers?: unknown, body?: unknown): Promise<ResponseBody>;
    patch(url: string, headers?: unknown, body?: unknown): Promise<ResponseBody>;
}

declare class SharedPreferences {
    get(key: string): string;
    getString(key: string, defaultValue?: string): string;
    setString(key: string, value: string): void;
}

declare class Element {
    key: string;
    text: string;
    outerHtml: string;
    innerHtml: string;
    className: string;
    localName: string;
    namespaceUri: string;
    getSrc: string;
    getImg: string;
    getHref: string;
    getDataSrc: string;
    previousElementSibling: Element;
    nextElementSibling: Element;
    children: Element[];
    getElementsByTagName(name: string): Element[];
    getElementsByClassName(name: string): Element[];
    selectFirst(selector: string): Element;
    select(selector: string): Element[];
    getString(type: string): string;
    getElementSibling(type: string): Element;
    xpath(xpath: string): string[];
    xpathFirst(xpath: string): string;
    attr(attr: string): string;
    hasAttr(attr: string): string;
}

declare class Document {
    constructor(html: string);
    html: string;
    body: Element;
    documentElement: Element;
    head: Element;
    parent: Element;
    text: string;
    outerHtml: string;
    children: Element[];
    getElementsByTagName(name: string): Element[];
    getElementsByClassName(name: string): Element[];
    getElementById(id: string): Element;
    selectFirst(selector: string): Element;
    select(selector: string): Element[];
    xpath(xpath: string): string[];
    xpathFirst(xpath: string): string;
    attr(attr: string): string;
    hasAttr(attr: string): string;
}

declare class MProvider {
    readonly source: MSource;
    readonly supportsLatest: boolean;
    getHeaders(url: string): Record<string, string>;
    getPopular(page: number): Promise<unknown>;
    getLatestUpdates(page: number): Promise<unknown>;
    search(query: string, page: number, filters: unknown): Promise<unknown>;
    getDetail(url: string): Promise<unknown>;
    getPageList(url: string): Promise<unknown>;
    getVideoList(url: string): Promise<unknown>;
    getHtmlContent(name: string, url: string): Promise<unknown>;
    cleanHtmlContent(html: string): Promise<unknown>;
    getFilterList(): unknown;
    getSourcePreferences(): unknown;
}
