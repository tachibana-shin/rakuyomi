// console, forwarded to the host log. Ported from the LNReader plugin
// runtime (`lnreader_js/src/webapi.ts` `consoleFormat`), adapted to the
// MangaYomi `sendMessage` bridge (`"log"` takes the formatted message).

function consoleFormat(v: unknown): string {
    if (typeof v === "string") return v;
    if (v instanceof Error) return v.message;
    try {
        return JSON.stringify(v);
    } catch (e) {
        return String(v);
    }
}

function logHost(args: unknown[]): void {
    sendMessage("log", JSON.stringify([args.map(consoleFormat).join(" ")]));
}

export const consoleObj: Record<
    "log" | "info" | "warn" | "error" | "debug",
    (...args: unknown[]) => void
> = {
    log: (...args: unknown[]) => logHost(args),
    info: (...args: unknown[]) => logHost(args),
    warn: (...args: unknown[]) => logHost(args),
    error: (...args: unknown[]) => logHost(args),
    debug: (...args: unknown[]) => logHost(args),
};
