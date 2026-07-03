## [1.36.6](https://github.com/tachibana-shin/rakuyomi/compare/v1.36.5...v1.36.6) (2026-07-03)


### Bug Fixes

* close_range file not found lua ([3886625](https://github.com/tachibana-shin/rakuyomi/commit/3886625127572e50a555c55a9fe1fb83beeda155))

## [1.36.5](https://github.com/tachibana-shin/rakuyomi/compare/v1.36.4...v1.36.5) (2026-07-02)


### Bug Fixes

* **platform:** close FDs in child processes ([#216](https://github.com/tachibana-shin/rakuyomi/issues/216)) ([f53c2f2](https://github.com/tachibana-shin/rakuyomi/commit/f53c2f2d6eaf1c75862be06ae269b7d9ad591cd0))

## [1.36.4](https://github.com/tachibana-shin/rakuyomi/compare/v1.36.3...v1.36.4) (2026-06-29)


### Performance Improvements

* maintain hideTopClose state when refreshing LibraryView after callbacks ([8b31fa9](https://github.com/tachibana-shin/rakuyomi/commit/8b31fa973094e43907fde394927008c943ca7f5f))

## [1.36.3](https://github.com/tachibana-shin/rakuyomi/compare/v1.36.2...v1.36.3) (2026-06-29)


### Performance Improvements

* add hideTopClose option to LibraryView and refactor backend initialization logic ([8d4337f](https://github.com/tachibana-shin/rakuyomi/commit/8d4337f9a2980c17be7b7f215298403091d42d8e))

## [1.36.2](https://github.com/tachibana-shin/rakuyomi/compare/v1.36.1...v1.36.2) (2026-06-28)


### Performance Improvements

* add file path support to chapters to enable direct access to preloaded content ([ff5c85b](https://github.com/tachibana-shin/rakuyomi/commit/ff5c85b288b59c9ee325be24d4a04e60ede420db))


### Reverts

* Revert "fix(manga-reader): apply file manager override to zen UI ([#198](https://github.com/tachibana-shin/rakuyomi/issues/198))" ([012fff7](https://github.com/tachibana-shin/rakuyomi/commit/012fff7ac4f1f31865330888f6f69ef05185b8d5))

## [1.36.1](https://github.com/tachibana-shin/rakuyomi/compare/v1.36.0...v1.36.1) (2026-06-27)


### Bug Fixes

* **l10n:** add update-trans Makefile target ([93eb38c](https://github.com/tachibana-shin/rakuyomi/commit/93eb38cb8f1a0203508f0f6cc7a5874b3cfb50cc))

# [1.36.0](https://github.com/tachibana-shin/rakuyomi/compare/v1.35.2...v1.36.0) (2026-06-27)


### Features

* Add backward navigation through chapters ([#212](https://github.com/tachibana-shin/rakuyomi/issues/212)) ([b22523e](https://github.com/tachibana-shin/rakuyomi/commit/b22523e30219ec373d560b5ded0d48fe653a3c6d))
* add configurable visibility settings for title and metadata in grid mode ([#211](https://github.com/tachibana-shin/rakuyomi/issues/211)) ([4b6cb10](https://github.com/tachibana-shin/rakuyomi/commit/4b6cb10206500b0ca1d2105999628cdc79ac23fa))
* add mode write to ram for protect emmc ([#213](https://github.com/tachibana-shin/rakuyomi/issues/213)) ([9d883a9](https://github.com/tachibana-shin/rakuyomi/commit/9d883a9f28527d8501b7176223d1e175357a6408))

## [1.35.2](https://github.com/tachibana-shin/rakuyomi/compare/v1.35.1...v1.35.2) (2026-06-25)


### Performance Improvements

* optimize server ([#210](https://github.com/tachibana-shin/rakuyomi/issues/210)) ([8917d5e](https://github.com/tachibana-shin/rakuyomi/commit/8917d5ee27ba7365d7cd7b09c32a2afab3e01805))

## [1.35.1](https://github.com/tachibana-shin/rakuyomi/compare/v1.35.0...v1.35.1) (2026-06-25)


### Bug Fixes

* callback assignment for zen home tab item ([#208](https://github.com/tachibana-shin/rakuyomi/issues/208)) ([4b6d1d0](https://github.com/tachibana-shin/rakuyomi/commit/4b6d1d0e253635e303c35f481dd7ace418539330))

# [1.35.0](https://github.com/tachibana-shin/rakuyomi/compare/v1.34.1...v1.35.0) (2026-06-19)


### Bug Fixes

* **manga-reader:** apply file manager override to zen UI ([#198](https://github.com/tachibana-shin/rakuyomi/issues/198)) ([215f224](https://github.com/tachibana-shin/rakuyomi/commit/215f2245d0487a37a9d697aee49ca676b2f73455))
* OTA update never shows the "Restart Now" dialog on old Kindles ([#187](https://github.com/tachibana-shin/rakuyomi/issues/187)) ([f38596e](https://github.com/tachibana-shin/rakuyomi/commit/f38596e81e6c38c87b2b4d427b7a69568de27160))


### Features

* **download:** add chapter download progress ([#197](https://github.com/tachibana-shin/rakuyomi/issues/197)) ([a61a2d9](https://github.com/tachibana-shin/rakuyomi/commit/a61a2d9d3d9d6939eb77c4869fe4b4830a513d5f))
* **logging:** add option to disable plugin logging ([#195](https://github.com/tachibana-shin/rakuyomi/issues/195)) ([161f44a](https://github.com/tachibana-shin/rakuyomi/commit/161f44a660c22070f2d74a5da23c10e17857543e))
* luacheck ([#199](https://github.com/tachibana-shin/rakuyomi/issues/199)) ([63b0412](https://github.com/tachibana-shin/rakuyomi/commit/63b041223cf7fbf249195e68736a374e44f756d7))
* **server:** add auto-stop server on rakuyomi close ([#196](https://github.com/tachibana-shin/rakuyomi/issues/196)) ([afd5d83](https://github.com/tachibana-shin/rakuyomi/commit/afd5d836acab5bfdfb0bf6be3032f95b047056d5))


### Performance Improvements

* **process:** Use FFI for binary execution ([#202](https://github.com/tachibana-shin/rakuyomi/issues/202)) ([98dd669](https://github.com/tachibana-shin/rakuyomi/commit/98dd669434197de37d4dbf2912f1ef402120f4dc))
