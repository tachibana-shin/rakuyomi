// String helpers MangaYomi JS extensions rely on (ported from the app's
// `eval/javascript` bridge classes; semantics kept verbatim).

declare global {
    interface String {
        substringAfter(pattern: string): string;
        substringAfterLast(pattern: string): string;
        substringBefore(pattern: string): string;
        substringBeforeLast(pattern: string): string;
        substringBetween(left: string, right: string): string;
    }
}

export function installStringHelpers(): void {
    const proto = String.prototype as unknown as Record<string, unknown>;
    proto.substringAfter = function (this: string, pattern: string): string {
        const startIndex = this.indexOf(pattern);
        if (startIndex === -1) return this.substring(0);

        const start = startIndex + pattern.length;
        return this.substring(start);
    };
    proto.substringAfterLast = function (this: string, pattern: string): string {
        return this.split(pattern).pop() ?? "";
    };
    proto.substringBefore = function (this: string, pattern: string): string {
        const endIndex = this.indexOf(pattern);
        if (endIndex === -1) return this.substring(0);

        return this.substring(0, endIndex);
    };
    proto.substringBeforeLast = function (this: string, pattern: string): string {
        const endIndex = this.lastIndexOf(pattern);
        if (endIndex === -1) return this.substring(0);
        return this.substring(0, endIndex);
    };
    proto.substringBetween = function (this: string, left: string, right: string): string {
        let startIndex = 0;
        let index = this.indexOf(left, startIndex);
        if (index === -1) return "";
        let leftIndex = index + left.length;
        let rightIndex = this.indexOf(right, leftIndex);
        if (rightIndex === -1) return "";
        startIndex = rightIndex + right.length;
        return this.substring(leftIndex, rightIndex);
    };
}
