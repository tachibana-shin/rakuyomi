# rakuyomi

**rakuyomi** is a manga reader plugin for [KOReader](https://github.com/koreader/koreader).

> [!IMPORTANT]
>
> The original author of the [@hanatsumi](https://github.com/hanatsumi) project no longer uses the e-link reader, so I am authorized to maintain this branch as the official branch.
>
> Thank [@hanatsumi](https://github.com/hanatsumi) for the great work!!
> 
> `rakuyomi` currently supports all [Aidoku](https://github.com/Aidoku) sources including sources written with legacy SDK or next SDK ([Aidoku Community Sources](https://github.com/Aidoku-Community/sources), [Tachibana Shin Sources](https://github.com/tachibana-shin/aidoku-community-sources)...)

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
- Details manga
- Aidoku source new SDK (0.7) support

<table>
  <tr>
    <td><img width="200" alt="image" src="https://github.com/user-attachments/assets/cccf4076-79d3-4eb2-af80-62266b0e7ede" /></td>
    <td><img src="https://github.com/user-attachments/assets/4edec936-8034-4467-bdce-5b060d799967" width="200" /></td>
    <td><img src="https://github.com/user-attachments/assets/1c5d29fe-5a2f-4bf9-8437-bd1afe1b4e8b" width="200" /></td>
  </tr>
<tr>
  <td><img src="https://github.com/user-attachments/assets/89397e28-65ae-46dc-b36f-dfc1d5e5b78c" width="200" /></td>
  <td><img src="https://github.com/user-attachments/assets/e71a161f-ffa0-4109-b602-3c36d3fddc2f" width="200" /></td>
  <td><img src="https://github.com/user-attachments/assets/36957374-89fb-490c-bea9-f52f750ca1d9" width="200" /></td>
</tr>
</table>
<em>Open source for Every One</em>

<p align="center">
    <img src="docs/src/images/demo.gif" width="60%" />
    <br/>
    <em><small><a href="https://seotch.wordpress.com/ubunchu/">"Ubunchu!"</a> by Hiroshi Seo is licensed under <a href="https://creativecommons.org/licenses/by-nc/3.0/">CC BY-NC 3.0</a>.</small></em>
</p>

> ![TIP]
>
> This fork currently supports my Light Novel sources: [tachibana-shin/aidoku-community-sources](https://github.com/tachibana-shin/aidoku-community-sources)
>
> Now support plugin image DRM

## Installation & Usage

For detailed installation and usage instructions, please check out the [Installation](https://tachibana-shin.github.io/rakuyomi/user-guide/installation/README.html) and [Quickstart](https://tachibana-shin.github.io/rakuyomi/user-guide/quickstart) sections on our user guide!

## Contributing

For information on how to contribute to rakuyomi, please check out the [Setting up the Environment](https://tachibana-shin.github.io/rakuyomi/contributing/setting-up-the-environment.html) section on our guide!

