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
  </tbody>
</table>

## reMarkable

Users of the **reMarkable Paper Pro** should use the **AArch64** build. Other reMarkable devices should work with the **Kindle** build.

## Kobo, PocketBook and other ARM e-readers

Use the **Kindle** build.

## BOOX and other Android-based e-readers

Android e-readers are currently unsupported. Leave a thumbs up on [this issue](https://github.com/hanatsumi/rakuyomi/issues/111) if you'd like for it to be supported!
