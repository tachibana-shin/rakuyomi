# Choosing a Build

In order to install rakuyomi, you'll first need to choose the correct build for your e-reader device. Read the section according to your device, and then download the appropriate build of the [latest release](https://github.com/tachibana-shin/rakuyomi/releases/latest). With the correct build in hand, proceed to [install it to your device](./user-guide/installation/installing-to-your-device).

If your device is unsupported or does not work with the given builds, feel free to open an issue on the [issue tracker](https://github.com/tachibana-shin/rakuyomi/issues)!

## Kindle

For Kindle devices, check the table below for determining the correct build:

<table>
  <thead>
    <tr>
      <th style="text-align: center">Model</th>
      <th style="text-align: center">Firmware Version</th>
      <th style="text-align: center">Build to Use</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td style="text-align: center">3rd generation or older<br/>(devices with physical keyboards)</td>
      <td style="text-align: center">any</td>
      <td style="text-align: center"><em>unsupported</em></td>
    </tr>
    <tr>
      <td style="text-align: center" rowspan="2">Kindle 4 or newer<br/>(devices <em>without</em> a physical keyboard)</td>
      <td style="text-align: center">≥ 5.16.3</td>
      <td style="text-align: center">Kindle (hard floats)</td>
    </tr>
    <tr>
      <td style="text-align: center">< 5.16.3</td>
      <td style="text-align: center">Kindle</td>
    </tr>
    <tr>
      <td style="text-align: center">Kindle CPU Cortex A9 (Kindle Paperwhite 3rd gen or older, Kindle Voyage 6th gen or older, Kindle 7th gen or older, Kindle Oasis 1st gen)</td>
      <td style="text-align: center">any</td>
      <td style="text-align: center">Kindle (Cortex-A9 optimized)</td>
    </tr>
  </tbody>
</table>

## reMarkable

Users of the **reMarkable Paper Pro** should use the **AArch64** build. Other reMarkable devices should work with the **Kindle** build.

## Kobo, PocketBook and other ARM e-readers

Use the **Kindle** build.

## BOOX and other Android-based e-readers

Android e-readers are now supported (Android 4.3+ / API 18+). Android builds require two components:

| Component | Description |
|---|---|
| `rakuyomi.koplugin` | Standard plugin package for KOReader |
| `RakuyomiBridge.apk` / `RakuyomiBridgeHeadless.apk` | Companion app that runs the rakuyomi server |

The companion app (`RakuyomiBridge.apk` / `RakuyomiBridgeHeadless.apk`) runs the rakuyomi HTTP server on `http://127.0.0.1:8787`. The KOReader plugin connects to it automatically.

### Installation steps

1. Install the normal plugin: follow the [installing guide](./user-guide/installation/installing-to-your-device) — place the `rakuyomi.koplugin` folder into KOReader's plugin directory.
2. Sideload `RakuyomiBridge.apk` / `RakuyomiBridgeHeadless.apk` from the [releases page](https://github.com/tachibana-shin/rakuyomi_bridge/releases/latest). (`RakuyomiBridge.exe` Abdroid 5+)
3. Open the **Rakuyomi Bridge** app on your device.
4. Grant the **"All files access"** permission when prompted (required for Android 11+).
5. Tap **"Start Server"** in the app, or enable **"Start on boot"**.
6. Open KOReader and launch rakuyomi as usual. The plugin will connect automatically.

> **Note**: Both KOReader and the companion app share the same data directory (`/storage/emulated/0/koreader/rakuyomi/`), so sources, downloads, and databases are stored in a single location.

---

### ⚠️ Important Note for Xiaomi Devices (MIUI / HyperOS)

To ensure **Rakuyomi Bridge** maintains a stable background connection and is not aggressively terminated by the system, you must adjust the following settings in the **App Info** page of the Rakuyomi Bridge app:

*   **Disable App Hibernation:** Turn off the **"Pause app activity if unused"** option.
*   **Adjust Battery Settings:** Change the Battery Saver profile to **"No restrictions"**.
