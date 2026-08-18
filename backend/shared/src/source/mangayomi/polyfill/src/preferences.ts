// The app's `SharedPreferences` (eval/javascript/b_shared_preferences.dart):
// get/set on the host-side preference map, keyed per extension.

export class SharedPreferences {
    get(key: string): string {
        return sendMessage("get", JSON.stringify([key]));
    }

    getString(key: string, defaultValue?: string): string {
        return sendMessage("getString", JSON.stringify([key, defaultValue]));
    }

    setString(key: string, value: string): void {
        sendMessage("setString", JSON.stringify([key, value]));
    }
}
