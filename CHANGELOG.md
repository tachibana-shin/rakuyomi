## [1.41.1](https://github.com/tachibana-shin/rakuyomi/compare/v1.41.0...v1.41.1) (2026-08-23)


### Bug Fixes

* disable stream mode ([11e7d37](https://github.com/tachibana-shin/rakuyomi/commit/11e7d37613ce462e66ea4e8d72f8c5f465138e88))


### Reverts

* remove streaming reader from main (still in development on feat/stream-read) ([92bb8df](https://github.com/tachibana-shin/rakuyomi/commit/92bb8df93c6a3155758d77be003b59e0c0fb50c8))

# [1.41.0](https://github.com/tachibana-shin/rakuyomi/compare/v1.40.3...v1.41.0) (2026-08-23)


### Features

* streaming chapter reader + dexvm protobuf support ([50e85a0](https://github.com/tachibana-shin/rakuyomi/commit/50e85a037fa1545a74058bede67c7941b044751d))

## [1.40.3](https://github.com/tachibana-shin/rakuyomi/compare/v1.40.2...v1.40.3) (2026-08-21)


### Bug Fixes

* crash when saving settings with a large tracker/chat id ([#317](https://github.com/tachibana-shin/rakuyomi/issues/317)) ([87a36bc](https://github.com/tachibana-shin/rakuyomi/commit/87a36bc803a6e003f1f17b290ebb503409d4c6c0)), closes [#316](https://github.com/tachibana-shin/rakuyomi/issues/316)
* record the SDK mode the Aidoku module actually booted as ([#311](https://github.com/tachibana-shin/rakuyomi/issues/311)) ([e2536a6](https://github.com/tachibana-shin/rakuyomi/commit/e2536a667eaed34abe59ada2ce117e31b98810c9)), closes [#304](https://github.com/tachibana-shin/rakuyomi/issues/304)
* search and library missing post text ([#308](https://github.com/tachibana-shin/rakuyomi/issues/308)) ([e79d298](https://github.com/tachibana-shin/rakuyomi/commit/e79d298043520b576d2fff27785e9351dd8c93dd))

## [1.40.2](https://github.com/tachibana-shin/rakuyomi/compare/v1.40.1...v1.40.2) (2026-08-19)


### Bug Fixes

* link kindle server binary fully static ([#303](https://github.com/tachibana-shin/rakuyomi/issues/303)) ([6413d14](https://github.com/tachibana-shin/rakuyomi/commit/6413d14427cebbef7e0c6cab9f6696ac83bb15f9))
* **release:** republish android plugin as a single rakuyomi-android.zip ([#302](https://github.com/tachibana-shin/rakuyomi/issues/302)) ([de4f3a4](https://github.com/tachibana-shin/rakuyomi/commit/de4f3a4c6bea8e5eccbf9e605cbc5486e0cdbdec)), closes [#296](https://github.com/tachibana-shin/rakuyomi/issues/296)

## [1.40.1](https://github.com/tachibana-shin/rakuyomi/compare/v1.40.0...v1.40.1) (2026-08-18)


### Bug Fixes

* condition for android build key in build-all.sh ([6e6e736](https://github.com/tachibana-shin/rakuyomi/commit/6e6e7369bf05638e09a3fc7decf2e488768d0c05))

# [1.40.0](https://github.com/tachibana-shin/rakuyomi/compare/v1.39.6...v1.40.0) (2026-08-18)


### Bug Fixes

* crash on tracking OAuth sign-in: Size.span.vertical_small does not exist ([#295](https://github.com/tachibana-shin/rakuyomi/issues/295)) ([5fcd0ee](https://github.com/tachibana-shin/rakuyomi/commit/5fcd0ee0e82ce7a1da14697fc1c08514c0631754))
* fetch CBZ metadata from the server instead of executing a binary ([#300](https://github.com/tachibana-shin/rakuyomi/issues/300)) ([35d4c35](https://github.com/tachibana-shin/rakuyomi/commit/35d4c358e3b030fc87aca5ace128156e8b798e11)), closes [#287](https://github.com/tachibana-shin/rakuyomi/issues/287)
* fix mangabaka tracking via API Key and OAuth2 ([#286](https://github.com/tachibana-shin/rakuyomi/issues/286)) ([0ad505e](https://github.com/tachibana-shin/rakuyomi/commit/0ad505ef83bc271a6502957fc87ee168c050fbb2))
* reconnect to Wi-Fi before retrying a failed chapter download ([#299](https://github.com/tachibana-shin/rakuyomi/issues/299)) ([97afb7c](https://github.com/tachibana-shin/rakuyomi/commit/97afb7cdd89534141a68e5397d0e87c33a9eb4a1)), closes [#277](https://github.com/tachibana-shin/rakuyomi/issues/277)
* **tracking:** include NSFW entries in MyAnimeList search ([#298](https://github.com/tachibana-shin/rakuyomi/issues/298)) ([212b082](https://github.com/tachibana-shin/rakuyomi/commit/212b08249f8e5f31ba42cb38c941b1145f46b0e3))


### Features

* support extension LNReader, Mangayomi (js, dart), Tachiyomi/Mihon ([#296](https://github.com/tachibana-shin/rakuyomi/issues/296)) ([a2d95f1](https://github.com/tachibana-shin/rakuyomi/commit/a2d95f1fb0184e90412f38c21a1e70cd04ab5656))

## [1.39.6](https://github.com/tachibana-shin/rakuyomi/compare/v1.39.5...v1.39.6) (2026-07-31)


### Bug Fixes

* migrate MyAnimeList requests to OAuth authentication and improve… ([#276](https://github.com/tachibana-shin/rakuyomi/issues/276)) ([2c0738f](https://github.com/tachibana-shin/rakuyomi/commit/2c0738f00d4ae2842c589acaf0fdff4993fe00ec))

## [1.39.5](https://github.com/tachibana-shin/rakuyomi/compare/v1.39.4...v1.39.5) (2026-07-26)


### Bug Fixes

* preserve hideTopClose state when opening playlist view ([#269](https://github.com/tachibana-shin/rakuyomi/issues/269)) ([67ba198](https://github.com/tachibana-shin/rakuyomi/commit/67ba1988ab0755731e050c7c78cd0e34d51e5787))

## [1.39.4](https://github.com/tachibana-shin/rakuyomi/compare/v1.39.3...v1.39.4) (2026-07-22)


### Performance Improvements

* add darwin support ([#266](https://github.com/tachibana-shin/rakuyomi/issues/266)) ([3b6cc3c](https://github.com/tachibana-shin/rakuyomi/commit/3b6cc3cde9250fc9aa89bf47e511f9949152b35d))

## [1.39.3](https://github.com/tachibana-shin/rakuyomi/compare/v1.39.2...v1.39.3) (2026-07-21)


### Bug Fixes

* use local tracking_value_definitions instead of global table in TrackingSettings ([b67a994](https://github.com/tachibana-shin/rakuyomi/commit/b67a994003a05aa516405ea9b5067044cf6c3c0d))

## [1.39.2](https://github.com/tachibana-shin/rakuyomi/compare/v1.39.1...v1.39.2) (2026-07-21)


### Bug Fixes

* use lowercase serialization for TrackingService and decouple instance tracking definitions in UI ([e0db2fb](https://github.com/tachibana-shin/rakuyomi/commit/e0db2fbb96d753565f65e733a67f31188a391aaf))

## [1.39.1](https://github.com/tachibana-shin/rakuyomi/compare/v1.39.0...v1.39.1) (2026-07-20)


### Bug Fixes

* ensure bridge URL is correctly constructed by trimming trailing slashes from server URL ([d9e6c8f](https://github.com/tachibana-shin/rakuyomi/commit/d9e6c8f272e2e2dc32aa479856206d540b056e89))

# [1.39.0](https://github.com/tachibana-shin/rakuyomi/compare/v1.38.0...v1.39.0) (2026-07-19)


### Bug Fixes

* settings UI and error-handling robustness ([#256](https://github.com/tachibana-shin/rakuyomi/issues/256)) ([cf71152](https://github.com/tachibana-shin/rakuyomi/commit/cf711523f29cd93b643c7b3f4964c2438e6b6528))
* use confirmText field name to match official Aidoku schema ([#255](https://github.com/tachibana-shin/rakuyomi/issues/255)) ([2fd439b](https://github.com/tachibana-shin/rakuyomi/commit/2fd439b5c7c8b7865df1489cb47c37dd031ce68b))


### Features

* downloaded-storage statistics (GET /storage-stats + Settings display) ([#257](https://github.com/tachibana-shin/rakuyomi/issues/257)) ([1bb515a](https://github.com/tachibana-shin/rakuyomi/commit/1bb515ac47fe95419e634410e713bd9780438263))
* implement manga tracking integration with AniList and MyAnimeList.... ([#260](https://github.com/tachibana-shin/rakuyomi/issues/260)) ([7b8cc3f](https://github.com/tachibana-shin/rakuyomi/commit/7b8cc3f9160bded632fc896f7c6270d414623f17))
* optional auto-delete of downloaded chapters ([#258](https://github.com/tachibana-shin/rakuyomi/issues/258)) ([81f2d1a](https://github.com/tachibana-shin/rakuyomi/commit/81f2d1a0eade873602532a708f3439f810f5d0fd))

# [1.38.0](https://github.com/tachibana-shin/rakuyomi/compare/v1.37.2...v1.38.0) (2026-07-15)


### Features

* add configurable chapter title format for ComicInfo.xml metadata ([#253](https://github.com/tachibana-shin/rakuyomi/issues/253)) ([ba56169](https://github.com/tachibana-shin/rakuyomi/commit/ba561696fb6db8173b557f2c085bb6df5fe7f42e))
* add top-zone tap/swipe to open KOReader native top bar across a… ([#252](https://github.com/tachibana-shin/rakuyomi/issues/252)) ([8432777](https://github.com/tachibana-shin/rakuyomi/commit/8432777878e1c477711af097bcee20d0c398fd26))

## [1.37.2](https://github.com/tachibana-shin/rakuyomi/compare/v1.37.1...v1.37.2) (2026-07-14)


### Performance Improvements

* add test cases to rust ([#248](https://github.com/tachibana-shin/rakuyomi/issues/248)) ([cecd3be](https://github.com/tachibana-shin/rakuyomi/commit/cecd3be2f65237cea0319f2ad54aa72038cde0a7))

## [1.37.1](https://github.com/tachibana-shin/rakuyomi/compare/v1.37.0...v1.37.1) (2026-07-14)


### Bug Fixes

* **tls:** use owned ClientConfig for use_preconfigured_tls and route … ([#246](https://github.com/tachibana-shin/rakuyomi/issues/246)) ([ac8c74a](https://github.com/tachibana-shin/rakuyomi/commit/ac8c74a0559feb3163203d90de5883e732491271))

# [1.37.0](https://github.com/tachibana-shin/rakuyomi/compare/v1.36.11...v1.37.0) (2026-07-13)


### Features

* add new js apis from aidoku-rs SDK ([#238](https://github.com/tachibana-shin/rakuyomi/issues/238)) ([09a972d](https://github.com/tachibana-shin/rakuyomi/commit/09a972d6c6942249b35a01c474580d22a774ff2e))
* implement Telegram bot for cookie management ([#233](https://github.com/tachibana-shin/rakuyomi/issues/233)) ([148a069](https://github.com/tachibana-shin/rakuyomi/commit/148a06930b1eb72476c07d830ac4f1f5ce82ed2a))
* **manga:** add per-manga viewer preference ([#241](https://github.com/tachibana-shin/rakuyomi/issues/241)) ([2553704](https://github.com/tachibana-shin/rakuyomi/commit/2553704bbc8bfbca868ecf9f2684e6091d463515))
* **proxy:** add global proxy support ([#239](https://github.com/tachibana-shin/rakuyomi/issues/239)) ([21a73ae](https://github.com/tachibana-shin/rakuyomi/commit/21a73aef5a65235f12f2a23839761fd1d380ab14))


### Performance Improvements

* **unix:** replace fork with posix_spawn ([#242](https://github.com/tachibana-shin/rakuyomi/issues/242)) ([cef20bc](https://github.com/tachibana-shin/rakuyomi/commit/cef20bc1af2b14af645b33dce301696788975ec0))

## [1.36.11](https://github.com/tachibana-shin/rakuyomi/compare/v1.36.10...v1.36.11) (2026-07-09)


### Bug Fixes

* correct property name for on_return_callback in MangaSearchResults.lua ([#229](https://github.com/tachibana-shin/rakuyomi/issues/229)) ([686d809](https://github.com/tachibana-shin/rakuyomi/commit/686d809d5dd9315925cfab79fad08ab22304e8ae))


### Performance Improvements

* implement navigation to specific manga and chapters via file metadata and refactor backend state management ([#231](https://github.com/tachibana-shin/rakuyomi/issues/231)) ([f33e750](https://github.com/tachibana-shin/rakuyomi/commit/f33e750ac94fa178473188ca85cb6415f13395e8))

## [1.36.10](https://github.com/tachibana-shin/rakuyomi/compare/v1.36.9...v1.36.10) (2026-07-09)


### Bug Fixes

* replace system TLS with manual rustls implementation for ce… ([#225](https://github.com/tachibana-shin/rakuyomi/issues/225)) ([f5a8bd2](https://github.com/tachibana-shin/rakuyomi/commit/f5a8bd24e30e2b2915f561ce9702af38d5a5a518))

## [1.36.9](https://github.com/tachibana-shin/rakuyomi/compare/v1.36.8...v1.36.9) (2026-07-07)


### Bug Fixes

* resolve race conditions by capturing chapter ID during preloadin… ([#218](https://github.com/tachibana-shin/rakuyomi/issues/218)) ([0eb10ff](https://github.com/tachibana-shin/rakuyomi/commit/0eb10ff52e0cb28e41748f12fc9f7923b3e8e33a))


### Performance Improvements

* revert fix fork because koreader fixed ([#221](https://github.com/tachibana-shin/rakuyomi/issues/221)) ([dcdb820](https://github.com/tachibana-shin/rakuyomi/commit/dcdb8201b7101e646ae92a4c612093585c4add19)), closes [#216](https://github.com/tachibana-shin/rakuyomi/issues/216)

## [1.36.8](https://github.com/tachibana-shin/rakuyomi/compare/v1.36.7...v1.36.8) (2026-07-07)


### Bug Fixes

* method call to use Shared namespace ([487e396](https://github.com/tachibana-shin/rakuyomi/commit/487e3967df880f884a10c4c3996387c5f8e59a43))

## [1.36.7](https://github.com/tachibana-shin/rakuyomi/compare/v1.36.6...v1.36.7) (2026-07-06)


### Performance Improvements

* update Rust dependencies, implement ZIP comment metadata for chapter origin, and enforce SQL query safety ([#219](https://github.com/tachibana-shin/rakuyomi/issues/219)) ([3f3b2f4](https://github.com/tachibana-shin/rakuyomi/commit/3f3b2f4b384ef61a473c87ae39d6236118566ba4))

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
