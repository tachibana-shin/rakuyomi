# Installing to Your Device

After downloading the plugin, follow these instructions to install it on your device.

## Writing Your `settings.json`

The `settings.json` file contains basic settings that rakuyomi needs to work, including:
- **Source lists**: URLs containing information about available sources
- **Languages**: Your preferred reading languages

A recommended starter configuration is created automatically on the first plugin start, so writing this file by hand is optional. The defaults are equivalent to the snippet below — use it as a reference if you want to customise the file before that first run.

Any source that can run on [Aidoku](https://github.com/Aidoku) can also run on [rakuyomi](https://github.com/tachibana-shin/rakuyomi) (except `WebView`)

```json,downloadable:settings.json
{{#include ../../../../backend/server/assets/default-settings.json}}
```

## Copying the Plugin to Your Device

1. Extract the `.zip` file containing the plugin. You should find a `rakuyomi.koplugin` folder inside.
2. Connect your e-reader to your computer.
3. Navigate to your KOReader installation folder. Common locations include:
   - **Cervantes:** `/mnt/private/koreader`
   - **Kindle:** `koreader/`
   - **Kobo:** `.adds/koreader/`
   - **PocketBook:** `applications/koreader/`

4. Locate the `plugins` folder:
![koreader folder](./user-guide/installation/images/koreader-folder.png)

5. Copy the entire `rakuyomi.koplugin` folder into the `plugins` folder:
![plugins folder](./user-guide/installation/images/plugins-folder.png)

```admonish note
Steps 6 and 7 are only needed if you customised the snippet above. Otherwise, skip ahead — rakuyomi creates the `rakuyomi` folder and a default `settings.json` automatically on first start.
```

6. Return to the KOReader folder and create a new `rakuyomi` folder:
![rakuyomi folder](./user-guide/installation/images/rakuyomi-folder.png)

7. Copy your `settings.json` file into the new `rakuyomi` folder:
![settings file](./user-guide/installation/images/settings-file.png)

## Android Devices

Android (BOOX, Onyx, etc.) requires an extra step — an active **companion app** is needed to run the rakuyomi server, because Android does not allow KOReader to launch arbitrary binaries directly.

1. Follow the normal steps above to copy `rakuyomi.koplugin` into KOReader's `plugins` folder.
2. Sideload `RakuyomiBridge.apk` from the [releases page](https://github.com/tachibana-shin/rakuyomi/releases/latest). You can do this via:
   - **ADB**: `adb install RakuyomiBridge.apk`
   - A file manager app on the device
3. Open the **Rakuyomi Bridge** app.
4. Grant the **"All files access"** special permission if your device runs Android 11 or newer (Settings → Apps → Special access → All files access).
5. Tap **"Start Server"**. The app will run a foreground service with a persistent notification.
6. Open KOReader. The rakuyomi plugin will detect the companion app automatically.

> The companion app shares the same data directory (`/storage/emulated/0/koreader/rakuyomi/`) as KOReader's rakuyomi plugin. Books downloaded through the companion app are immediately available in KOReader, and vice-versa
.
### ⚠️ Important Note for Xiaomi Devices (MIUI / HyperOS)

To ensure **Rakuyomi Bridge** maintains a stable background connection and is not aggressively terminated by the system, you must adjust the following settings in the **App Info** page of the Rakuyomi Bridge app:

*   **Disable App Hibernation:** Turn off the **"Pause app activity if unused"** option.
*   **Adjust Battery Settings:** Change the Battery Saver profile to **"No restrictions"**.

rakuyomi is now installed on your device! Ready to get started? Check out the [Quickstart Guide](../quickstart.md) to learn how to use it effectively.