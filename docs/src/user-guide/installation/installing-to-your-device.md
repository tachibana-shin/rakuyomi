# Installing to Your Device

After downloading the plugin, follow these instructions to install it on your device.

## Writing Your `settings.json`

The `settings.json` file contains basic settings that rakuyomi needs to work, including:
- **Source lists**: URLs containing information about available sources
- **Languages**: Your preferred reading languages

Here's a recommended starter configuration that you can customize or use as-is:

Any source that can run on [Aidoku](https://github.com/Aidoku) can also run on [rakuyomi](https://github.com/tachibana-shin/rakuyomi) (except `WebView`)

```json,downloadable:settings.json
{
  "$schema": "https://github.com/tachibana-shin/rakuyomi/releases/latest/download/settings.schema.json",
  "source_lists": [
    "https://raw.githubusercontent.com/tachibana-shin/aidoku-community-sources/gh-pages/index.min.json",
    
    "https://aidoku-community.github.io/sources/index.min.json"
  ],
  "languages": ["en"]
}
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

6. Return to the KOReader folder and create a new `rakuyomi` folder:
![rakuyomi folder](./user-guide/installation/images/rakuyomi-folder.png)

7. Copy your `settings.json` file into the new `rakuyomi` folder:
![settings file](./user-guide/installation/images/settings-file.png)

rakuyomi is now installed on your device! Ready to get started? Check out the [Quickstart Guide](../quickstart.md) to learn how to use it effectively.