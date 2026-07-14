# Changelog

## [0.6.1](https://github.com/hiro-o918/rinkaku/compare/v0.6.0...v0.6.1) (2026-07-14)


### Features

* classify Rust *_tests/ directories as test paths ([#130](https://github.com/hiro-o918/rinkaku/issues/130)) ([129de60](https://github.com/hiro-o918/rinkaku/commit/129de60089b15bb45e6d9d7b9f4f7a4490a76a54)), closes [#114](https://github.com/hiro-o918/rinkaku/issues/114)
* diff-style marker prefixes and referenced class for mermaid output (ADR 0041) ([#133](https://github.com/hiro-o918/rinkaku/issues/133)) ([1b7f695](https://github.com/hiro-o918/rinkaku/commit/1b7f6954d71680f98ef4a8e0bc75340f4e1a87ce))
* exclude test referrers from fan-in and encode review risk in the TUI ([#137](https://github.com/hiro-o918/rinkaku/issues/137)) ([eacb845](https://github.com/hiro-o918/rinkaku/commit/eacb8453b852acc4b224880f825d3df6fc4c1f6b))
* rework mermaid visual encoding and legend (ADR 0039) ([#128](https://github.com/hiro-o918/rinkaku/issues/128)) ([5e0e44b](https://github.com/hiro-o918/rinkaku/commit/5e0e44b25645ecd5c4274f5d247ca393b59a7625))
* tighten file size thresholds and always show per-file line counts ([#123](https://github.com/hiro-o918/rinkaku/issues/123)) ([e6ff1d0](https://github.com/hiro-o918/rinkaku/commit/e6ff1d0009b5e03102eaf491ba402e1d2dbd5983))


### Bug Fixes

* **core:** resolve Rust scoped_identifier path references (e.g. enum variant paths) ([#126](https://github.com/hiro-o918/rinkaku/issues/126)) ([b8aaedf](https://github.com/hiro-o918/rinkaku/commit/b8aaedf48332d6c777b928d1409b1a2bc5d2ba4b))


### Miscellaneous

* release main ([#119](https://github.com/hiro-o918/rinkaku/issues/119)) ([957c4d4](https://github.com/hiro-o918/rinkaku/commit/957c4d4a00cada5f01d6a0df54a5a2cc20b8451d))

## [0.6.0](https://github.com/hiro-o918/rinkaku/compare/v0.5.0...v0.6.0) (2026-07-13)


### ⚠ BREAKING CHANGES

* the Markdown "## Hotspots" heading is now "## High fan-in symbols"; the JSON "hotspots" field is now "fan_ins"; the Mermaid "hotspot" class is now "fan-in".

### refactor

* rename hotspot vocabulary to fan-in (ADR 0033) ([#101](https://github.com/hiro-o918/rinkaku/issues/101)) ([d8dcc0f](https://github.com/hiro-o918/rinkaku/commit/d8dcc0f29dba399f933c91cfd3693376c61f3b77))


### Features

* surface file-size warnings in rinkaku output (ADR 0028) ([#86](https://github.com/hiro-o918/rinkaku/issues/86)) ([00c7c5a](https://github.com/hiro-o918/rinkaku/commit/00c7c5a7aef57f18eaa30acc0c03775af26b54c2))
* tighten file size thresholds and always show per-file line counts ([#123](https://github.com/hiro-o918/rinkaku/issues/123)) ([e6ff1d0](https://github.com/hiro-o918/rinkaku/commit/e6ff1d0009b5e03102eaf491ba402e1d2dbd5983))
* **tui:** add a startup splash screen with real progress (ADR 0033) ([#100](https://github.com/hiro-o918/rinkaku/issues/100)) ([38673bc](https://github.com/hiro-o918/rinkaku/commit/38673bcc42450acc2bca132ae770fdbc6ffb310a))
* **tui:** report per-file progress during diff analysis ([#107](https://github.com/hiro-o918/rinkaku/issues/107)) ([b590601](https://github.com/hiro-o918/rinkaku/commit/b590601a748dfaaf8a69c709d42b236546b71f3c))


### Bug Fixes

* **core:** add id tie-break to compute_hotspots for full determinism ([#96](https://github.com/hiro-o918/rinkaku/issues/96)) ([89d4b56](https://github.com/hiro-o918/rinkaku/commit/89d4b5630696eb1e12c6d00fc5c60fd596dcfbc7))
* unify release-please into a single-PR release cycle ([#73](https://github.com/hiro-o918/rinkaku/issues/73)) ([796d249](https://github.com/hiro-o918/rinkaku/commit/796d249ee7067ede9207dcd54a643e028f0e123a))


### Documentation

* renumber duplicate ADR 0029 to 0031 ([#90](https://github.com/hiro-o918/rinkaku/issues/90)) ([79cafbe](https://github.com/hiro-o918/rinkaku/commit/79cafbe49a66227f26ddcd53430dbee6f090229d))
* renumber duplicate ADR 0033 (fan-in rename) to 0034 ([#105](https://github.com/hiro-o918/rinkaku/issues/105)) ([e4d6f70](https://github.com/hiro-o918/rinkaku/commit/e4d6f703f06a6bc8fdd99ac6ddadabd7bd97e114))


### Miscellaneous

* release main ([#50](https://github.com/hiro-o918/rinkaku/issues/50)) ([8791d00](https://github.com/hiro-o918/rinkaku/commit/8791d00edeba0f84d7b028ce46a05e7f4ec0c178))
* release main ([#74](https://github.com/hiro-o918/rinkaku/issues/74)) ([5e0986f](https://github.com/hiro-o918/rinkaku/commit/5e0986f3048457ccee5283a3e7770fc149c7e4c7))
* release main ([#76](https://github.com/hiro-o918/rinkaku/issues/76)) ([e1b7943](https://github.com/hiro-o918/rinkaku/commit/e1b7943d2df1a12320f1d1fe4c29814a1b635c1f))
* release main ([#77](https://github.com/hiro-o918/rinkaku/issues/77)) ([b8d0393](https://github.com/hiro-o918/rinkaku/commit/b8d039320ef8e3f0455a4fc2bdadf5ba8e2440e6))

## [0.5.0](https://github.com/hiro-o918/rinkaku/compare/v0.4.0...v0.5.0) (2026-07-13)


### ⚠ BREAKING CHANGES

* the Markdown "## Hotspots" heading is now "## High fan-in symbols"; the JSON "hotspots" field is now "fan_ins"; the Mermaid "hotspot" class is now "fan-in".
* any script or CI invocation of `rinkaku --include-tests` now fails to parse and must switch to omitting the flag (new default) or to `--exclude-tests` (previous default's exclusion). The tool has not shipped a stable CLI yet.

### refactor

* rename hotspot vocabulary to fan-in (ADR 0033) ([#101](https://github.com/hiro-o918/rinkaku/issues/101)) ([d8dcc0f](https://github.com/hiro-o918/rinkaku/commit/d8dcc0f29dba399f933c91cfd3693376c61f3b77))


### Features

* classify changed symbols by contract impact ([#45](https://github.com/hiro-o918/rinkaku/issues/45)) ([b8a8b37](https://github.com/hiro-o918/rinkaku/commit/b8a8b37ab6af44fae5e6bdc2db0891f3d7f545d0))
* condense change-graph rendering for human readability ([#40](https://github.com/hiro-o918/rinkaku/issues/40)) ([f3308e8](https://github.com/hiro-o918/rinkaku/commit/f3308e8013c63e5eb720cd9c6cb58cdf593a82c1))
* default to including test symbols; rename --include-tests to --exclude-tests ([#69](https://github.com/hiro-o918/rinkaku/issues/69)) ([56a98b9](https://github.com/hiro-o918/rinkaku/commit/56a98b98c365902d8b8c355a1f1ea8ff5660248f))
* detect generated files by content markers and drop them from output ([#39](https://github.com/hiro-o918/rinkaku/issues/39)) ([506f62e](https://github.com/hiro-o918/rinkaku/commit/506f62ef8565401a56c2c464a9b696e86aed2e0e))
* entry-path pivot — re-root the graph at a chosen path ([#56](https://github.com/hiro-o918/rinkaku/issues/56)) ([470f85b](https://github.com/hiro-o918/rinkaku/commit/470f85b99b242f93241ce5a27af4976629ad0b2f))
* exclude test symbols and generated files from output by default ([#38](https://github.com/hiro-o918/rinkaku/issues/38)) ([44d4e3b](https://github.com/hiro-o918/rinkaku/commit/44d4e3bee9e96bd0e366fe7fd75ce41d87e6f898))
* mermaid output format and PR report GitHub Action ([#59](https://github.com/hiro-o918/rinkaku/issues/59)) ([b706067](https://github.com/hiro-o918/rinkaku/commit/b70606719d1427d8cb69ff007ab2b43251528ad2))
* render output as entry-point trees over the changed-symbol graph ([#35](https://github.com/hiro-o918/rinkaku/issues/35)) ([1b53bdc](https://github.com/hiro-o918/rinkaku/commit/1b53bdc4e72d99fe45703cfd989aba1b9e021082))
* surface fan-in hotspots in rendered output ([#43](https://github.com/hiro-o918/rinkaku/issues/43)) ([b38cf86](https://github.com/hiro-o918/rinkaku/commit/b38cf867fd197e897229bd580d31708457368c59))
* surface file-size warnings in rinkaku output (ADR 0028) ([#86](https://github.com/hiro-o918/rinkaku/issues/86)) ([00c7c5a](https://github.com/hiro-o918/rinkaku/commit/00c7c5a7aef57f18eaa30acc0c03775af26b54c2))
* **tui:** add a startup splash screen with real progress (ADR 0033) ([#100](https://github.com/hiro-o918/rinkaku/issues/100)) ([38673bc](https://github.com/hiro-o918/rinkaku/commit/38673bcc42450acc2bca132ae770fdbc6ffb310a))
* **tui:** report per-file progress during diff analysis ([#107](https://github.com/hiro-o918/rinkaku/issues/107)) ([b590601](https://github.com/hiro-o918/rinkaku/commit/b590601a748dfaaf8a69c709d42b236546b71f3c))
* **tui:** show skipped and test-only files in the entry tree ([#58](https://github.com/hiro-o918/rinkaku/issues/58)) ([e4c21d3](https://github.com/hiro-o918/rinkaku/commit/e4c21d3b2f4fb8c461c63ad9b10c96fcb279c6da))
* whole-repo outline as the default input mode ([#52](https://github.com/hiro-o918/rinkaku/issues/52)) ([b8f551a](https://github.com/hiro-o918/rinkaku/commit/b8f551a24c02684b880bfe7071f1073cb8509d38))


### Bug Fixes

* **core:** add id tie-break to compute_hotspots for full determinism ([#96](https://github.com/hiro-o918/rinkaku/issues/96)) ([89d4b56](https://github.com/hiro-o918/rinkaku/commit/89d4b5630696eb1e12c6d00fc5c60fd596dcfbc7))
* unify release-please into a single-PR release cycle ([#73](https://github.com/hiro-o918/rinkaku/issues/73)) ([796d249](https://github.com/hiro-o918/rinkaku/commit/796d249ee7067ede9207dcd54a643e028f0e123a))


### Documentation

* renumber duplicate ADR 0029 to 0031 ([#90](https://github.com/hiro-o918/rinkaku/issues/90)) ([79cafbe](https://github.com/hiro-o918/rinkaku/commit/79cafbe49a66227f26ddcd53430dbee6f090229d))
* renumber duplicate ADR 0033 (fan-in rename) to 0034 ([#105](https://github.com/hiro-o918/rinkaku/issues/105)) ([e4d6f70](https://github.com/hiro-o918/rinkaku/commit/e4d6f703f06a6bc8fdd99ac6ddadabd7bd97e114))


### Miscellaneous

* **main:** release rinkaku-core 0.2.0 ([#37](https://github.com/hiro-o918/rinkaku/issues/37)) ([e724dff](https://github.com/hiro-o918/rinkaku/commit/e724dff2e386668d95736a4688833f97d1fd1222))
* release main ([#50](https://github.com/hiro-o918/rinkaku/issues/50)) ([8791d00](https://github.com/hiro-o918/rinkaku/commit/8791d00edeba0f84d7b028ce46a05e7f4ec0c178))
* release main ([#74](https://github.com/hiro-o918/rinkaku/issues/74)) ([5e0986f](https://github.com/hiro-o918/rinkaku/commit/5e0986f3048457ccee5283a3e7770fc149c7e4c7))
* release main ([#76](https://github.com/hiro-o918/rinkaku/issues/76)) ([e1b7943](https://github.com/hiro-o918/rinkaku/commit/e1b7943d2df1a12320f1d1fe4c29814a1b635c1f))

## [0.4.0](https://github.com/hiro-o918/rinkaku/compare/v0.3.0...v0.4.0) (2026-07-13)


### ⚠ BREAKING CHANGES

* any script or CI invocation of `rinkaku --include-tests` now fails to parse and must switch to omitting the flag (new default) or to `--exclude-tests` (previous default's exclusion). The tool has not shipped a stable CLI yet.

### Features

* classify changed symbols by contract impact ([#45](https://github.com/hiro-o918/rinkaku/issues/45)) ([b8a8b37](https://github.com/hiro-o918/rinkaku/commit/b8a8b37ab6af44fae5e6bdc2db0891f3d7f545d0))
* condense change-graph rendering for human readability ([#40](https://github.com/hiro-o918/rinkaku/issues/40)) ([f3308e8](https://github.com/hiro-o918/rinkaku/commit/f3308e8013c63e5eb720cd9c6cb58cdf593a82c1))
* default to including test symbols; rename --include-tests to --exclude-tests ([#69](https://github.com/hiro-o918/rinkaku/issues/69)) ([56a98b9](https://github.com/hiro-o918/rinkaku/commit/56a98b98c365902d8b8c355a1f1ea8ff5660248f))
* detect generated files by content markers and drop them from output ([#39](https://github.com/hiro-o918/rinkaku/issues/39)) ([506f62e](https://github.com/hiro-o918/rinkaku/commit/506f62ef8565401a56c2c464a9b696e86aed2e0e))
* entry-path pivot — re-root the graph at a chosen path ([#56](https://github.com/hiro-o918/rinkaku/issues/56)) ([470f85b](https://github.com/hiro-o918/rinkaku/commit/470f85b99b242f93241ce5a27af4976629ad0b2f))
* exclude test symbols and generated files from output by default ([#38](https://github.com/hiro-o918/rinkaku/issues/38)) ([44d4e3b](https://github.com/hiro-o918/rinkaku/commit/44d4e3bee9e96bd0e366fe7fd75ce41d87e6f898))
* mermaid output format and PR report GitHub Action ([#59](https://github.com/hiro-o918/rinkaku/issues/59)) ([b706067](https://github.com/hiro-o918/rinkaku/commit/b70606719d1427d8cb69ff007ab2b43251528ad2))
* render output as entry-point trees over the changed-symbol graph ([#35](https://github.com/hiro-o918/rinkaku/issues/35)) ([1b53bdc](https://github.com/hiro-o918/rinkaku/commit/1b53bdc4e72d99fe45703cfd989aba1b9e021082))
* surface fan-in hotspots in rendered output ([#43](https://github.com/hiro-o918/rinkaku/issues/43)) ([b38cf86](https://github.com/hiro-o918/rinkaku/commit/b38cf867fd197e897229bd580d31708457368c59))
* **tui:** show skipped and test-only files in the entry tree ([#58](https://github.com/hiro-o918/rinkaku/issues/58)) ([e4c21d3](https://github.com/hiro-o918/rinkaku/commit/e4c21d3b2f4fb8c461c63ad9b10c96fcb279c6da))
* whole-repo outline as the default input mode ([#52](https://github.com/hiro-o918/rinkaku/issues/52)) ([b8f551a](https://github.com/hiro-o918/rinkaku/commit/b8f551a24c02684b880bfe7071f1073cb8509d38))


### Bug Fixes

* unify release-please into a single-PR release cycle ([#73](https://github.com/hiro-o918/rinkaku/issues/73)) ([796d249](https://github.com/hiro-o918/rinkaku/commit/796d249ee7067ede9207dcd54a643e028f0e123a))


### Miscellaneous

* **main:** release rinkaku-core 0.2.0 ([#37](https://github.com/hiro-o918/rinkaku/issues/37)) ([e724dff](https://github.com/hiro-o918/rinkaku/commit/e724dff2e386668d95736a4688833f97d1fd1222))
* release main ([#50](https://github.com/hiro-o918/rinkaku/issues/50)) ([8791d00](https://github.com/hiro-o918/rinkaku/commit/8791d00edeba0f84d7b028ce46a05e7f4ec0c178))
* release main ([#74](https://github.com/hiro-o918/rinkaku/issues/74)) ([5e0986f](https://github.com/hiro-o918/rinkaku/commit/5e0986f3048457ccee5283a3e7770fc149c7e4c7))

## [0.3.0](https://github.com/hiro-o918/rinkaku/compare/v0.2.1...v0.3.0) (2026-07-13)


### ⚠ BREAKING CHANGES

* any script or CI invocation of `rinkaku --include-tests` now fails to parse and must switch to omitting the flag (new default) or to `--exclude-tests` (previous default's exclusion). The tool has not shipped a stable CLI yet.

### Features

* add Go language support ([09fd76e](https://github.com/hiro-o918/rinkaku/commit/09fd76e1c118ccf9d7ce5927dc5799ddc93b2bb8))
* add LanguageSupport trait and language registry ([bb6a73e](https://github.com/hiro-o918/rinkaku/commit/bb6a73e32dc07b153497faea049ff9fc48dccb82))
* add Python language support ([31a66c9](https://github.com/hiro-o918/rinkaku/commit/31a66c95290f5f197e766cb8ada8a68a686bcc93))
* add reference queries to language support ([a70a686](https://github.com/hiro-o918/rinkaku/commit/a70a686123a0387f2ef21fef8eb0e4ee974d9df6))
* add report rendering in Markdown and JSON ([bfc99ea](https://github.com/hiro-o918/rinkaku/commit/bfc99ea7bead707d166ff3e03d0b15d8a332a87e))
* add tags-based dependency resolver ([e73775a](https://github.com/hiro-o918/rinkaku/commit/e73775a7038889c8c0f1737fb8ddac3615e52a0c))
* add tree-sitter based signature extraction with Rust support ([2507bc5](https://github.com/hiro-o918/rinkaku/commit/2507bc5700af810afbea21d0fbac428d534c950b))
* add TypeScript language support ([94ea67e](https://github.com/hiro-o918/rinkaku/commit/94ea67e1a88fa485981e2a4ef14bb0e24aafbcb4))
* add unified diff parser ([cd7a5c7](https://github.com/hiro-o918/rinkaku/commit/cd7a5c7ffbda50fb8c44c075f75c7a6123b9fb87))
* classify changed symbols by contract impact ([#45](https://github.com/hiro-o918/rinkaku/issues/45)) ([b8a8b37](https://github.com/hiro-o918/rinkaku/commit/b8a8b37ab6af44fae5e6bdc2db0891f3d7f545d0))
* condense change-graph rendering for human readability ([#40](https://github.com/hiro-o918/rinkaku/issues/40)) ([f3308e8](https://github.com/hiro-o918/rinkaku/commit/f3308e8013c63e5eb720cd9c6cb58cdf593a82c1))
* default to including test symbols; rename --include-tests to --exclude-tests ([#69](https://github.com/hiro-o918/rinkaku/issues/69)) ([56a98b9](https://github.com/hiro-o918/rinkaku/commit/56a98b98c365902d8b8c355a1f1ea8ff5660248f))
* detect generated files by content markers and drop them from output ([#39](https://github.com/hiro-o918/rinkaku/issues/39)) ([506f62e](https://github.com/hiro-o918/rinkaku/commit/506f62ef8565401a56c2c464a9b696e86aed2e0e))
* entry-path pivot — re-root the graph at a chosen path ([#56](https://github.com/hiro-o918/rinkaku/issues/56)) ([470f85b](https://github.com/hiro-o918/rinkaku/commit/470f85b99b242f93241ce5a27af4976629ad0b2f))
* exclude test symbols and generated files from output by default ([#38](https://github.com/hiro-o918/rinkaku/issues/38)) ([44d4e3b](https://github.com/hiro-o918/rinkaku/commit/44d4e3bee9e96bd0e366fe7fd75ce41d87e6f898))
* handle copy from/to headers ([f453c14](https://github.com/hiro-o918/rinkaku/commit/f453c142303ccb437f811ddfad01e092b4ce8eb6))
* mermaid output format and PR report GitHub Action ([#59](https://github.com/hiro-o918/rinkaku/issues/59)) ([b706067](https://github.com/hiro-o918/rinkaku/commit/b70606719d1427d8cb69ff007ab2b43251528ad2))
* render dependencies and add --deps flag ([8e55d9c](https://github.com/hiro-o918/rinkaku/commit/8e55d9c99a1ebb5ed6cdc130a328a661d00609ac))
* render output as entry-point trees over the changed-symbol graph ([#35](https://github.com/hiro-o918/rinkaku/issues/35)) ([1b53bdc](https://github.com/hiro-o918/rinkaku/commit/1b53bdc4e72d99fe45703cfd989aba1b9e021082))
* surface fan-in hotspots in rendered output ([#43](https://github.com/hiro-o918/rinkaku/issues/43)) ([b38cf86](https://github.com/hiro-o918/rinkaku/commit/b38cf867fd197e897229bd580d31708457368c59))
* **tui:** show skipped and test-only files in the entry tree ([#58](https://github.com/hiro-o918/rinkaku/issues/58)) ([e4c21d3](https://github.com/hiro-o918/rinkaku/commit/e4c21d3b2f4fb8c461c63ad9b10c96fcb279c6da))
* whole-repo outline as the default input mode ([#52](https://github.com/hiro-o918/rinkaku/issues/52)) ([b8f551a](https://github.com/hiro-o918/rinkaku/commit/b8f551a24c02684b880bfe7071f1073cb8509d38))
* wire CLI entrypoint with stdin and git diff input ([a8ce8e3](https://github.com/hiro-o918/rinkaku/commit/a8ce8e3770c452f57eb5d2a2b8284977567dd567))
* wire diff parsing, language lookup, and extraction into a pipeline ([a1c463b](https://github.com/hiro-o918/rinkaku/commit/a1c463bb488728830c90ab0de5708f32a8a0918e))


### Bug Fixes

* compile reference queries once per file instead of per symbol ([be49687](https://github.com/hiro-o918/rinkaku/commit/be49687a1eae5668f16c0f94429f75ff3b7e0545))
* declare explicit crate versions for release-please compatibility ([6002680](https://github.com/hiro-o918/rinkaku/commit/600268047ae2f36dd7bd0ad96f3a1d7fda4fc9ab))
* drop underscore and single-char identifiers from referenced names ([10f77fd](https://github.com/hiro-o918/rinkaku/commit/10f77fd52e98f15a7d31bfb428da4797f5718448))
* exclude diff-local symbols from dependencies by name and path ([250d32b](https://github.com/hiro-o918/rinkaku/commit/250d32b0e15b565c1e7b6b604fd6e5583134a0f3))
* harden Markdown rendering against fences and use ? over unwrap ([41895eb](https://github.com/hiro-o918/rinkaku/commit/41895eb6ad8fa73494b9a51420a099b5a6cef7fa))
* prevent integer overflow on malformed hunk headers ([2b1ec69](https://github.com/hiro-o918/rinkaku/commit/2b1ec697531a949be85e0c66e718d97c2f2522e2))
* rank same-name dependency matches by path proximity and cap at 3 ([58b9b34](https://github.com/hiro-o918/rinkaku/commit/58b9b346bbfe0338a04315e050b206646b1f3e6c))
* read --base mode files via git show instead of the working tree ([9d061a9](https://github.com/hiro-o918/rinkaku/commit/9d061a91678a336871480561681d231999ae3f4b))
* reject hunk markers that do not match the expected @@ prefix ([660e343](https://github.com/hiro-o918/rinkaku/commit/660e3430e82d0ae71460f44cbd3ed362cf52e93d))
* reject hunks whose body does not match the declared line count ([383ab10](https://github.com/hiro-o918/rinkaku/commit/383ab1018cfed9046808972ede32c26edef859c4))
* skip read_file for pure renames and mode-change-only diffs ([3a85d58](https://github.com/hiro-o918/rinkaku/commit/3a85d58ee9789ae9213a3a667d006418bc80ab1d))
* support TS abstract classes and class field arrow function bodies ([e902d02](https://github.com/hiro-o918/rinkaku/commit/e902d02787f2c9c6b5612b710b54ac4f87109188))
* unify release-please into a single-PR release cycle ([#73](https://github.com/hiro-o918/rinkaku/issues/73)) ([796d249](https://github.com/hiro-o918/rinkaku/commit/796d249ee7067ede9207dcd54a643e028f0e123a))
* warn on stdin input that produces zero recognized file changes ([1ccc183](https://github.com/hiro-o918/rinkaku/commit/1ccc18327dc952970c23279db24d4e1f4252d417))


### Documentation

* document proximity ranking edge cases and expect() safety ([7282351](https://github.com/hiro-o918/rinkaku/commit/72823511c84241991855378dbdb5fdcaaf06b175))
* fix stale doc comment on extract_git_header_paths ([82d9dc8](https://github.com/hiro-o918/rinkaku/commit/82d9dc83884fd326573cf4f1ec7341cb8df55c8f))
* note that path headings are not Markdown-escaped ([feff4cf](https://github.com/hiro-o918/rinkaku/commit/feff4cf0bd462739743efda287d3bd44a5589e2f))
* reconcile const-bound arrow function wording with actual behavior ([2a5fb14](https://github.com/hiro-o918/rinkaku/commit/2a5fb145ef1e75432ac7937b3c13f4c957db3914))
* record measured effect and limits of the indexing prefilter ([4a34adb](https://github.com/hiro-o918/rinkaku/commit/4a34adb667b74f08ba27ee93ed7d67ba65f7091b))
* record the double-parse of changed files as a known inefficiency ([c6012de](https://github.com/hiro-o918/rinkaku/commit/c6012dead1e92623eb7c7d482d93704c233dbc21))


### Miscellaneous

* bootstrap cargo workspace with rinkaku-core crate ([e79bae4](https://github.com/hiro-o918/rinkaku/commit/e79bae41c2a8f83f3e8d0edb63ea0e9446435508))
* **main:** release rinkaku-core 0.2.0 ([#37](https://github.com/hiro-o918/rinkaku/issues/37)) ([e724dff](https://github.com/hiro-o918/rinkaku/commit/e724dff2e386668d95736a4688833f97d1fd1222))
* release main ([#14](https://github.com/hiro-o918/rinkaku/issues/14)) ([187c651](https://github.com/hiro-o918/rinkaku/commit/187c6517fc581850da9d79ea00cd3401aab08d1e))
* release main ([#50](https://github.com/hiro-o918/rinkaku/issues/50)) ([8791d00](https://github.com/hiro-o918/rinkaku/commit/8791d00edeba0f84d7b028ce46a05e7f4ec0c178))
* remove bootstrap sample function ([c2c3cc3](https://github.com/hiro-o918/rinkaku/commit/c2c3cc35606a77f1b161740e9dcaf1f9221c65a4))

## [0.2.1](https://github.com/hiro-o918/rinkaku/compare/rinkaku-core-v0.2.0...rinkaku-core-v0.2.1) (2026-07-13)


### Bug Fixes

* unify release-please into a single-PR release cycle ([#73](https://github.com/hiro-o918/rinkaku/issues/73)) ([796d249](https://github.com/hiro-o918/rinkaku/commit/796d249ee7067ede9207dcd54a643e028f0e123a))

## [0.2.0](https://github.com/hiro-o918/rinkaku/compare/rinkaku-core-v0.1.0...rinkaku-core-v0.2.0) (2026-07-13)


### ⚠ BREAKING CHANGES

* any script or CI invocation of `rinkaku --include-tests` now fails to parse and must switch to omitting the flag (new default) or to `--exclude-tests` (previous default's exclusion). The tool has not shipped a stable CLI yet.

### Features

* classify changed symbols by contract impact ([#45](https://github.com/hiro-o918/rinkaku/issues/45)) ([b8a8b37](https://github.com/hiro-o918/rinkaku/commit/b8a8b37ab6af44fae5e6bdc2db0891f3d7f545d0))
* condense change-graph rendering for human readability ([#40](https://github.com/hiro-o918/rinkaku/issues/40)) ([f3308e8](https://github.com/hiro-o918/rinkaku/commit/f3308e8013c63e5eb720cd9c6cb58cdf593a82c1))
* default to including test symbols; rename --include-tests to --exclude-tests ([#69](https://github.com/hiro-o918/rinkaku/issues/69)) ([56a98b9](https://github.com/hiro-o918/rinkaku/commit/56a98b98c365902d8b8c355a1f1ea8ff5660248f))
* detect generated files by content markers and drop them from output ([#39](https://github.com/hiro-o918/rinkaku/issues/39)) ([506f62e](https://github.com/hiro-o918/rinkaku/commit/506f62ef8565401a56c2c464a9b696e86aed2e0e))
* entry-path pivot — re-root the graph at a chosen path ([#56](https://github.com/hiro-o918/rinkaku/issues/56)) ([470f85b](https://github.com/hiro-o918/rinkaku/commit/470f85b99b242f93241ce5a27af4976629ad0b2f))
* exclude test symbols and generated files from output by default ([#38](https://github.com/hiro-o918/rinkaku/issues/38)) ([44d4e3b](https://github.com/hiro-o918/rinkaku/commit/44d4e3bee9e96bd0e366fe7fd75ce41d87e6f898))
* mermaid output format and PR report GitHub Action ([#59](https://github.com/hiro-o918/rinkaku/issues/59)) ([b706067](https://github.com/hiro-o918/rinkaku/commit/b70606719d1427d8cb69ff007ab2b43251528ad2))
* render output as entry-point trees over the changed-symbol graph ([#35](https://github.com/hiro-o918/rinkaku/issues/35)) ([1b53bdc](https://github.com/hiro-o918/rinkaku/commit/1b53bdc4e72d99fe45703cfd989aba1b9e021082))
* surface fan-in hotspots in rendered output ([#43](https://github.com/hiro-o918/rinkaku/issues/43)) ([b38cf86](https://github.com/hiro-o918/rinkaku/commit/b38cf867fd197e897229bd580d31708457368c59))
* **tui:** show skipped and test-only files in the entry tree ([#58](https://github.com/hiro-o918/rinkaku/issues/58)) ([e4c21d3](https://github.com/hiro-o918/rinkaku/commit/e4c21d3b2f4fb8c461c63ad9b10c96fcb279c6da))
* whole-repo outline as the default input mode ([#52](https://github.com/hiro-o918/rinkaku/issues/52)) ([b8f551a](https://github.com/hiro-o918/rinkaku/commit/b8f551a24c02684b880bfe7071f1073cb8509d38))

## 0.1.0 (2026-07-12)


### Features

* add Go language support ([09fd76e](https://github.com/hiro-o918/rinkaku/commit/09fd76e1c118ccf9d7ce5927dc5799ddc93b2bb8))
* add LanguageSupport trait and language registry ([bb6a73e](https://github.com/hiro-o918/rinkaku/commit/bb6a73e32dc07b153497faea049ff9fc48dccb82))
* add Python language support ([31a66c9](https://github.com/hiro-o918/rinkaku/commit/31a66c95290f5f197e766cb8ada8a68a686bcc93))
* add reference queries to language support ([a70a686](https://github.com/hiro-o918/rinkaku/commit/a70a686123a0387f2ef21fef8eb0e4ee974d9df6))
* add report rendering in Markdown and JSON ([bfc99ea](https://github.com/hiro-o918/rinkaku/commit/bfc99ea7bead707d166ff3e03d0b15d8a332a87e))
* add tags-based dependency resolver ([e73775a](https://github.com/hiro-o918/rinkaku/commit/e73775a7038889c8c0f1737fb8ddac3615e52a0c))
* add tree-sitter based signature extraction with Rust support ([2507bc5](https://github.com/hiro-o918/rinkaku/commit/2507bc5700af810afbea21d0fbac428d534c950b))
* add TypeScript language support ([94ea67e](https://github.com/hiro-o918/rinkaku/commit/94ea67e1a88fa485981e2a4ef14bb0e24aafbcb4))
* add unified diff parser ([cd7a5c7](https://github.com/hiro-o918/rinkaku/commit/cd7a5c7ffbda50fb8c44c075f75c7a6123b9fb87))
* handle copy from/to headers ([f453c14](https://github.com/hiro-o918/rinkaku/commit/f453c142303ccb437f811ddfad01e092b4ce8eb6))
* render dependencies and add --deps flag ([8e55d9c](https://github.com/hiro-o918/rinkaku/commit/8e55d9c99a1ebb5ed6cdc130a328a661d00609ac))
* wire CLI entrypoint with stdin and git diff input ([a8ce8e3](https://github.com/hiro-o918/rinkaku/commit/a8ce8e3770c452f57eb5d2a2b8284977567dd567))
* wire diff parsing, language lookup, and extraction into a pipeline ([a1c463b](https://github.com/hiro-o918/rinkaku/commit/a1c463bb488728830c90ab0de5708f32a8a0918e))


### Bug Fixes

* compile reference queries once per file instead of per symbol ([be49687](https://github.com/hiro-o918/rinkaku/commit/be49687a1eae5668f16c0f94429f75ff3b7e0545))
* declare explicit crate versions for release-please compatibility ([6002680](https://github.com/hiro-o918/rinkaku/commit/600268047ae2f36dd7bd0ad96f3a1d7fda4fc9ab))
* drop underscore and single-char identifiers from referenced names ([10f77fd](https://github.com/hiro-o918/rinkaku/commit/10f77fd52e98f15a7d31bfb428da4797f5718448))
* exclude diff-local symbols from dependencies by name and path ([250d32b](https://github.com/hiro-o918/rinkaku/commit/250d32b0e15b565c1e7b6b604fd6e5583134a0f3))
* harden Markdown rendering against fences and use ? over unwrap ([41895eb](https://github.com/hiro-o918/rinkaku/commit/41895eb6ad8fa73494b9a51420a099b5a6cef7fa))
* prevent integer overflow on malformed hunk headers ([2b1ec69](https://github.com/hiro-o918/rinkaku/commit/2b1ec697531a949be85e0c66e718d97c2f2522e2))
* rank same-name dependency matches by path proximity and cap at 3 ([58b9b34](https://github.com/hiro-o918/rinkaku/commit/58b9b346bbfe0338a04315e050b206646b1f3e6c))
* read --base mode files via git show instead of the working tree ([9d061a9](https://github.com/hiro-o918/rinkaku/commit/9d061a91678a336871480561681d231999ae3f4b))
* reject hunk markers that do not match the expected @@ prefix ([660e343](https://github.com/hiro-o918/rinkaku/commit/660e3430e82d0ae71460f44cbd3ed362cf52e93d))
* reject hunks whose body does not match the declared line count ([383ab10](https://github.com/hiro-o918/rinkaku/commit/383ab1018cfed9046808972ede32c26edef859c4))
* skip read_file for pure renames and mode-change-only diffs ([3a85d58](https://github.com/hiro-o918/rinkaku/commit/3a85d58ee9789ae9213a3a667d006418bc80ab1d))
* support TS abstract classes and class field arrow function bodies ([e902d02](https://github.com/hiro-o918/rinkaku/commit/e902d02787f2c9c6b5612b710b54ac4f87109188))
* warn on stdin input that produces zero recognized file changes ([1ccc183](https://github.com/hiro-o918/rinkaku/commit/1ccc18327dc952970c23279db24d4e1f4252d417))


### Documentation

* document proximity ranking edge cases and expect() safety ([7282351](https://github.com/hiro-o918/rinkaku/commit/72823511c84241991855378dbdb5fdcaaf06b175))
* fix stale doc comment on extract_git_header_paths ([82d9dc8](https://github.com/hiro-o918/rinkaku/commit/82d9dc83884fd326573cf4f1ec7341cb8df55c8f))
* note that path headings are not Markdown-escaped ([feff4cf](https://github.com/hiro-o918/rinkaku/commit/feff4cf0bd462739743efda287d3bd44a5589e2f))
* reconcile const-bound arrow function wording with actual behavior ([2a5fb14](https://github.com/hiro-o918/rinkaku/commit/2a5fb145ef1e75432ac7937b3c13f4c957db3914))
* record measured effect and limits of the indexing prefilter ([4a34adb](https://github.com/hiro-o918/rinkaku/commit/4a34adb667b74f08ba27ee93ed7d67ba65f7091b))
* record the double-parse of changed files as a known inefficiency ([c6012de](https://github.com/hiro-o918/rinkaku/commit/c6012dead1e92623eb7c7d482d93704c233dbc21))


### Miscellaneous

* bootstrap cargo workspace with rinkaku-core crate ([e79bae4](https://github.com/hiro-o918/rinkaku/commit/e79bae41c2a8f83f3e8d0edb63ea0e9446435508))
* remove bootstrap sample function ([c2c3cc3](https://github.com/hiro-o918/rinkaku/commit/c2c3cc35606a77f1b161740e9dcaf1f9221c65a4))
