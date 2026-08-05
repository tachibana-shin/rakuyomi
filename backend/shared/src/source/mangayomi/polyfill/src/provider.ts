// `MProvider` base class (eval/javascript/b_m_provider.dart) and the
// `jsonStringify` wrapper the host uses to invoke async extension methods.

export class MProvider {
    get source(): MSource {
        return RAKUYOMI_SOURCE;
    }
    get supportsLatest(): boolean {
        throw new Error("supportsLatest not implemented");
    }
    getHeaders(_url: string): Record<string, string> {
        throw new Error("getHeaders not implemented");
    }
    async getPopular(_page: number): Promise<unknown> {
        throw new Error("getPopular not implemented");
    }
    async getLatestUpdates(_page: number): Promise<unknown> {
        throw new Error("getLatestUpdates not implemented");
    }
    async search(_query: string, _page: number, _filters: unknown): Promise<unknown> {
        throw new Error("search not implemented");
    }
    async getDetail(_url: string): Promise<unknown> {
        throw new Error("getDetail not implemented");
    }
    async getPageList(_url: string): Promise<unknown> {
        throw new Error("getPageList not implemented");
    }
    async getVideoList(_url: string): Promise<unknown> {
        throw new Error("getVideoList not implemented");
    }
    async getHtmlContent(_name: string, _url: string): Promise<unknown> {
        throw new Error("getHtmlContent not implemented");
    }
    async cleanHtmlContent(_html: string): Promise<unknown> {
        throw new Error("cleanHtmlContent not implemented");
    }
    getFilterList(): unknown {
        throw new Error("getFilterList not implemented");
    }
    getSourcePreferences(): unknown {
        throw new Error("getSourcePreferences not implemented");
    }
}

export async function jsonStringify(fn: () => unknown): Promise<string> {
    return JSON.stringify(await fn());
}
