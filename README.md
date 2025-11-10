# rakuyomi

**rakuyomi** is a manga reader plugin for [KOReader](https://github.com/koreader/koreader).

This fork:
- Added last read time for manga, chapter
- Improved UI now you can see exactly which source manga belongs to
- Added cancel download methods to avoid freezing
- Fixed back button not working properly
- Added processing menu in chapter list
- Added "continue read"
- Correct write processing to save RAM to avoid freezing KoReader
- Added cleaner to free up memory
- Improved SQLite query method to speed up all operations by `200 times` including: library access, search, chapter list (`x300 times`)

<p align="center">
    <img src="docs/src/images/demo.gif" width="60%" />
    <br/>
    <em><small><a href="https://seotch.wordpress.com/ubunchu/">"Ubunchu!"</a> by Hiroshi Seo is licensed under <a href="https://creativecommons.org/licenses/by-nc/3.0/">CC BY-NC 3.0</a>.</small></em>
</p>

> [!TIP]
>
> This fork currently supports my Light Novel sources: [tachibana-shin/aidoku-community-sources](https://github.com/tachibana-shin/aidoku-community-sources)
>
> Now support plugin image DRM

## Installation & Usage

For detailed installation and usage instructions, please check out the [Installation](https://hanatsumi.github.io/rakuyomi/user-guide/installation/) and [Quickstart](https://hanatsumi.github.io/rakuyomi/user-guide/quickstart) sections on our user guide!

## Contributing

For information on how to contribute to rakuyomi, please check out the [Setting up the Environment](https://hanatsumi.github.io/rakuyomi/contributing/setting-up-the-environment.html) section on our guide!

