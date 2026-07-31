// @libs/storage -- mirrors the LNReader app's Storage/LocalStorage/SessionStorage.
// Instances are namespaced per plugin: the app constructs `new Storage(pluginId)`
// and keys are stored as `<pluginId>_DB_<key>` (MMKV in the app, host map here).

interface StorageItem {
  created: string;
  value: unknown;
  expires?: number;
}

export class Storage {
  private namespace: string;

  constructor(namespace: string) {
    this.namespace = namespace;
  }

  private key(key: string): string {
    return this.namespace + "_DB_" + String(key);
  }

  set(key: string, value: unknown, expires?: Date | number): void {
    const item: StorageItem = { created: new Date().toISOString(), value };
    if (expires instanceof Date) item.expires = expires.getTime();
    else if (typeof expires === "number") item.expires = expires;
    __rakuyomiStorageSet(this.key(key), JSON.stringify(item));
  }

  get(key: string, raw?: boolean): unknown {
    const s = __rakuyomiStorageGet(this.key(key));
    if (s === null) return undefined;
    let item: StorageItem;
    try {
      item = JSON.parse(s) as StorageItem;
    } catch (e) {
      return undefined;
    }
    if (item.expires && Date.now() > item.expires) {
      this.delete(key);
      return undefined;
    }
    if (raw) {
      return {
        created: item.created,
        value: item.value,
        expires: item.expires,
      };
    }
    return item.value;
  }

  delete(key: string): void {
    __rakuyomiStorageRemove(this.key(key));
  }

  clearAll(): void {
    const keys = JSON.parse(__rakuyomiStorageKeys()) as string[];
    const prefix = this.namespace + "_DB_";
    for (const k of keys) {
      if (k.indexOf(prefix) === 0) __rakuyomiStorageRemove(k);
    }
  }

  getAllKeys(): string[] {
    const keys = JSON.parse(__rakuyomiStorageKeys()) as string[];
    const prefix = this.namespace + "_DB_";
    const out: string[] = [];
    for (const k of keys) {
      if (k.indexOf(prefix) === 0) out.push(k.substring(prefix.length));
    }
    return out;
  }
}

export class LocalStorage {
  private namespace: string;

  constructor(namespace: string) {
    this.namespace = namespace;
  }

  get(): unknown {
    const s = __rakuyomiStorageGet(this.namespace + "_LocalStorage");
    if (s === null) return undefined;
    try {
      return JSON.parse(s);
    } catch (e) {
      return undefined;
    }
  }
}

export class SessionStorage {
  private namespace: string;

  constructor(namespace: string) {
    this.namespace = namespace;
  }

  get(): unknown {
    const s = __rakuyomiStorageGet(this.namespace + "_SessionStorage");
    if (s === null) return undefined;
    try {
      return JSON.parse(s);
    } catch (e) {
      return undefined;
    }
  }
}

export const storageApi = {
  storage: new Storage(__rakuyomiPluginId()),
  localStorage: new LocalStorage(__rakuyomiPluginId()),
  sessionStorage: new SessionStorage(__rakuyomiPluginId()),
};
