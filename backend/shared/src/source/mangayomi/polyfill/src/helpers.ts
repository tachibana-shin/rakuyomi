// Crypto and utility helpers MangaYomi JS extensions call as bare globals
// (ported from the app's `eval/javascript/*.dart` bridge classes). All the
// implementations live on the Rust side (`js/crypto.rs`), reached through
// `sendMessage`; these functions exist so extensions keep the exact calling
// convention of the app.

function sendCrypto(name: string, args: unknown[]): string {
    return sendMessage(name, JSON.stringify(args));
}

export const helperApi: Record<string, (...args: unknown[]) => string> = {
    cryptoHandler: (text, iv, secretKeyString, encrypt) =>
        sendCrypto("cryptoHandler", [text, iv, secretKeyString, encrypt]),
    encryptAESCryptoJS: (plainText, passphrase) =>
        sendCrypto("encryptAESCryptoJS", [plainText, passphrase]),
    decryptAESCryptoJS: (encrypted, passphrase) =>
        sendCrypto("decryptAESCryptoJS", [encrypted, passphrase]),
    decryptAESGCM: (encrypted, keyHex, ivHex, tagHex) =>
        sendCrypto("decryptAESGCM", [encrypted, keyHex, ivHex, tagHex]),
    deobfuscateJsPassword: (inputString) =>
        sendCrypto("deobfuscateJsPassword", [inputString]),
    unpackJsAndCombine: (scriptBlock) =>
        sendCrypto("unpackJsAndCombine", [scriptBlock]),
    unpackJs: (packedJS) => sendCrypto("unpackJs", [packedJS]),
    parseDates: (value, dateFormat, dateFormatLocale) =>
        sendCrypto("parseDates", [value, dateFormat, dateFormatLocale]),
};
