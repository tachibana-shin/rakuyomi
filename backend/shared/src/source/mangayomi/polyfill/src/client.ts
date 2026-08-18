// The app's `Client` (eval/javascript/b_client.dart): HTTP requests through
// the host bridge. Argument arrays mirror the app's bridge calls exactly
// (`get` carries no body element; the mutating methods do).

interface ResponseBody {
    body?: string;
    headers?: Record<string, string>;
    statusCode?: number;
}

export class Client {
    private readonly reqcopyWith: unknown;

    constructor(reqcopyWith?: unknown) {
        this.reqcopyWith = reqcopyWith ?? null;
    }

    async head(url: string, headers?: unknown): Promise<ResponseBody> {
        const result = await sendMessage(
            "http_head",
            JSON.stringify([null, this.reqcopyWith, url, headers])
        );
        return JSON.parse(result) as ResponseBody;
    }

    async get(url: string, headers?: unknown): Promise<ResponseBody> {
        const result = await sendMessage(
            "http_get",
            JSON.stringify([null, this.reqcopyWith, url, headers])
        );
        return JSON.parse(result) as ResponseBody;
    }

    async post(url: string, headers?: unknown, body?: unknown): Promise<ResponseBody> {
        const result = await sendMessage(
            "http_post",
            JSON.stringify([null, this.reqcopyWith, url, headers, body])
        );
        return JSON.parse(result) as ResponseBody;
    }

    async put(url: string, headers?: unknown, body?: unknown): Promise<ResponseBody> {
        const result = await sendMessage(
            "http_post",
            JSON.stringify([null, this.reqcopyWith, url, headers, body])
        );
        return JSON.parse(result) as ResponseBody;
    }

    async delete(url: string, headers?: unknown, body?: unknown): Promise<ResponseBody> {
        const result = await sendMessage(
            "http_post",
            JSON.stringify([null, this.reqcopyWith, url, headers, body])
        );
        return JSON.parse(result) as ResponseBody;
    }

    async patch(url: string, headers?: unknown, body?: unknown): Promise<ResponseBody> {
        const result = await sendMessage(
            "http_post",
            JSON.stringify([null, this.reqcopyWith, url, headers, body])
        );
        return JSON.parse(result) as ResponseBody;
    }
}
