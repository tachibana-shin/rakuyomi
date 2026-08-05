// Globals injected by the Rust side (backend/shared/src/source/lnreader/runtime.rs)
// before this script is evaluated, plus the synchronous host functions.
//
// The script is evaluated once per source worker inside the embedded JS engine
// (quickjs via rquickjs), in global scope. Everything here is provided by Rust.

declare const RAKUYOMI_PLUGIN_ID: string;
declare const RAKUYOMI_PLUGIN_SITE: string;
declare const RAKUYOMI_USER_AGENT: string;
declare const RAKUYOMI_PLUGIN_CODE: string;

declare function __rakuyomiFetch(url: string, initJson: string): string;
declare function __rakuyomiDecode(b64: string, encoding: string): string;
declare function __rakuyomiEncodeUtf8(str: string): string;
declare function __rakuyomiLog(level: string, message: string): void;
declare function __rakuyomiSleep(ms: number): void;
declare function __rakuyomiUuid(): string;
declare function __rakuyomiPluginId(): string;
declare function __rakuyomiStorageGet(key: string): string | null;
declare function __rakuyomiStorageSet(key: string, itemJson: string): void;
declare function __rakuyomiStorageRemove(key: string): void;
declare function __rakuyomiStorageClear(): void;
declare function __rakuyomiStorageKeys(): string;

// Native base64 helpers provided by the Rust host (runtime.rs); no JS shim.
declare function atob(data: string): string;
declare function btoa(data: string): string;
