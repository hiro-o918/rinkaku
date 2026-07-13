# Changelog

## [0.5.0](https://github.com/hiro-o918/rinkaku/compare/v0.4.0...v0.5.0) (2026-07-13)


### ⚠ BREAKING CHANGES

* the Markdown "## Hotspots" heading is now "## High fan-in symbols"; the JSON "hotspots" field is now "fan_ins"; the Mermaid "hotspot" class is now "fan-in".
* any script or CI invocation of `rinkaku --include-tests` now fails to parse and must switch to omitting the flag (new default) or to `--exclude-tests` (previous default's exclusion). The tool has not shipped a stable CLI yet.

### refactor

* rename hotspot vocabulary to fan-in (ADR 0033) ([#101](https://github.com/hiro-o918/rinkaku/issues/101)) ([d8dcc0f](https://github.com/hiro-o918/rinkaku/commit/d8dcc0f29dba399f933c91cfd3693376c61f3b77))


### Features

* add interactive TUI with directory-tree entry view ([#49](https://github.com/hiro-o918/rinkaku/issues/49)) ([29665e5](https://github.com/hiro-o918/rinkaku/commit/29665e5bf9e0395733088babc46bb5eebb56f566))
* default to including test symbols; rename --include-tests to --exclude-tests ([#69](https://github.com/hiro-o918/rinkaku/issues/69)) ([56a98b9](https://github.com/hiro-o918/rinkaku/commit/56a98b98c365902d8b8c355a1f1ea8ff5660248f))
* entry-path pivot — re-root the graph at a chosen path ([#56](https://github.com/hiro-o918/rinkaku/issues/56)) ([470f85b](https://github.com/hiro-o918/rinkaku/commit/470f85b99b242f93241ce5a27af4976629ad0b2f))
* surface file-size warnings in rinkaku output (ADR 0028) ([#86](https://github.com/hiro-o918/rinkaku/issues/86)) ([00c7c5a](https://github.com/hiro-o918/rinkaku/commit/00c7c5a7aef57f18eaa30acc0c03775af26b54c2))
* **tui:** add a startup splash screen with real progress (ADR 0033) ([#100](https://github.com/hiro-o918/rinkaku/issues/100)) ([38673bc](https://github.com/hiro-o918/rinkaku/commit/38673bcc42450acc2bca132ae770fdbc6ffb310a))
* **tui:** add diff pane and directory/file detail views ([#51](https://github.com/hiro-o918/rinkaku/issues/51)) ([23ae23b](https://github.com/hiro-o918/rinkaku/commit/23ae23b620950cb015451367b3012021293a0122))
* **tui:** add right-pane scrolling with an overflow indicator ([#54](https://github.com/hiro-o918/rinkaku/issues/54)) ([3dd002e](https://github.com/hiro-o918/rinkaku/commit/3dd002e0dc7259318d86e1f1a7c7d1270cb4d55f))
* **tui:** diff pane follows symbol selection with auto-scroll ([#80](https://github.com/hiro-o918/rinkaku/issues/80)) ([be8d412](https://github.com/hiro-o918/rinkaku/commit/be8d4126162d6f8a1a23e2db440ee8b401c2894d))
* **tui:** highlight the focused pane border ([#106](https://github.com/hiro-o918/rinkaku/issues/106)) ([30feaf3](https://github.com/hiro-o918/rinkaku/commit/30feaf3b256413c68725011a4f54aa013081aabe))
* **tui:** interaction model v2 — focus, default diff, scoped hunks, help overlay ([#61](https://github.com/hiro-o918/rinkaku/issues/61)) ([1edad34](https://github.com/hiro-o918/rinkaku/commit/1edad34c70c61b101c8763efb30a1e3438281f99))
* **tui:** jump to callers/callees with gd/gr and a jumplist ([#62](https://github.com/hiro-o918/rinkaku/issues/62)) ([c951555](https://github.com/hiro-o918/rinkaku/commit/c951555787d3e694d76229ec9036ce7f3774b678))
* **tui:** keep tests out of production directory ranking and add a trailing Tests section ([#112](https://github.com/hiro-o918/rinkaku/issues/112)) ([5746679](https://github.com/hiro-o918/rinkaku/commit/5746679e6fc6cfaaf59547d87b675775cd1685c4))
* **tui:** label the contract-change tree badge as api:N ([#97](https://github.com/hiro-o918/rinkaku/issues/97)) ([f90ef0f](https://github.com/hiro-o918/rinkaku/commit/f90ef0f2be4ccc1fd94322f70ce61d5af0eb7dbf))
* **tui:** make help overlay scrollable ([#85](https://github.com/hiro-o918/rinkaku/issues/85)) ([b9ac8af](https://github.com/hiro-o918/rinkaku/commit/b9ac8afee5b780cbb36b874f56e2f119eafec863))
* **tui:** rename the pivot pane to blast radius ([#64](https://github.com/hiro-o918/rinkaku/issues/64)) ([f8b188f](https://github.com/hiro-o918/rinkaku/commit/f8b188f67a9db2ece2c552afa8001f7f3eaf0e29))
* **tui:** scrollable source screen and half-page/top-bottom keys on the right pane ([#70](https://github.com/hiro-o918/rinkaku/issues/70)) ([84ba727](https://github.com/hiro-o918/rinkaku/commit/84ba7273762bfd80d4ea83ab36fe13c91250e999))
* **tui:** show skipped and test-only files in the entry tree ([#58](https://github.com/hiro-o918/rinkaku/issues/58)) ([e4c21d3](https://github.com/hiro-o918/rinkaku/commit/e4c21d3b2f4fb8c461c63ad9b10c96fcb279c6da))
* **tui:** support mouse wheel scrolling ([#84](https://github.com/hiro-o918/rinkaku/issues/84)) ([745385c](https://github.com/hiro-o918/rinkaku/commit/745385c2b519d46f507f0938ece7f422433bbd19))
* **tui:** sync tree cursor to symbol when diff pane is scrolled ([#89](https://github.com/hiro-o918/rinkaku/issues/89)) ([79e8e6f](https://github.com/hiro-o918/rinkaku/commit/79e8e6fd50aae07ee4221a5b213c253caea44e72))
* **tui:** syntax-highlight the diff pane via tree-sitter ([#55](https://github.com/hiro-o918/rinkaku/issues/55)) ([b314e79](https://github.com/hiro-o918/rinkaku/commit/b314e799d4a8949ca458e591c47f43866331f7b7))
* **tui:** syntax-highlight the source drill-down ([#68](https://github.com/hiro-o918/rinkaku/issues/68)) ([bd88f5d](https://github.com/hiro-o918/rinkaku/commit/bd88f5da0293cdb29eaf08bc3cb36e4a64791a64))
* whole-repo outline as the default input mode ([#52](https://github.com/hiro-o918/rinkaku/issues/52)) ([b8f551a](https://github.com/hiro-o918/rinkaku/commit/b8f551a24c02684b880bfe7071f1073cb8509d38))


### Bug Fixes

* bump rinkaku-core dep to 0.2.0 in rinkaku and rinkaku-tui ([#72](https://github.com/hiro-o918/rinkaku/issues/72)) ([886cb05](https://github.com/hiro-o918/rinkaku/commit/886cb05a76af602e9fa81166e05a74884fa3e711))
* **tui:** attribute a hunk to every intersecting symbol in the diff pane ([#87](https://github.com/hiro-o918/rinkaku/issues/87)) ([2d2a322](https://github.com/hiro-o918/rinkaku/commit/2d2a32259d4710c679aeac507ff853a2217737d3))
* **tui:** correct splash logo misspelling "rinkarku" to "rinkaku" ([#118](https://github.com/hiro-o918/rinkaku/issues/118)) ([169f978](https://github.com/hiro-o918/rinkaku/commit/169f978702bc329ed89523ec38b25e664eb4b06c))
* **tui:** drop DIM modifier from DarkGray-styled text for readability ([#104](https://github.com/hiro-o918/rinkaku/issues/104)) ([2fed439](https://github.com/hiro-o918/rinkaku/commit/2fed4399876a8d1a3fb30bea575f2951900f37f6))
* **tui:** keep jump popup candidates one row each so the window math holds ([#83](https://github.com/hiro-o918/rinkaku/issues/83)) ([e22a655](https://github.com/hiro-o918/rinkaku/commit/e22a6553c6bb517fa814fc76e81184778d9a5318))
* **tui:** open the TUI when the diff is piped via stdin ([#67](https://github.com/hiro-o918/rinkaku/issues/67)) ([62947ad](https://github.com/hiro-o918/rinkaku/commit/62947ade7a6130ec237458a8971bd089b2362abe))
* **tui:** resolve source view paths against the repository root ([#57](https://github.com/hiro-o918/rinkaku/issues/57)) ([0ff7c01](https://github.com/hiro-o918/rinkaku/commit/0ff7c01487844c66230ab3cfb50d1d4dcc4e245a))
* **tui:** restore Modifier import broken by concurrent merges ([#108](https://github.com/hiro-o918/rinkaku/issues/108)) ([4ff244b](https://github.com/hiro-o918/rinkaku/commit/4ff244b97d1b2347401a116a9aba147b448b36e5))
* **tui:** unify Enter on the diff pane and fix scroll behavior ([#65](https://github.com/hiro-o918/rinkaku/issues/65)) ([0de6f81](https://github.com/hiro-o918/rinkaku/commit/0de6f81546a918d89a6e458d0c2664eb2cef545d))
* unify release-please into a single-PR release cycle ([#73](https://github.com/hiro-o918/rinkaku/issues/73)) ([796d249](https://github.com/hiro-o918/rinkaku/commit/796d249ee7067ede9207dcd54a643e028f0e123a))


### Documentation

* renumber duplicate ADR 0033 (fan-in rename) to 0034 ([#105](https://github.com/hiro-o918/rinkaku/issues/105)) ([e4d6f70](https://github.com/hiro-o918/rinkaku/commit/e4d6f703f06a6bc8fdd99ac6ddadabd7bd97e114))


### Miscellaneous

* release main ([#50](https://github.com/hiro-o918/rinkaku/issues/50)) ([8791d00](https://github.com/hiro-o918/rinkaku/commit/8791d00edeba0f84d7b028ce46a05e7f4ec0c178))
* release main ([#74](https://github.com/hiro-o918/rinkaku/issues/74)) ([5e0986f](https://github.com/hiro-o918/rinkaku/commit/5e0986f3048457ccee5283a3e7770fc149c7e4c7))
* release main ([#76](https://github.com/hiro-o918/rinkaku/issues/76)) ([e1b7943](https://github.com/hiro-o918/rinkaku/commit/e1b7943d2df1a12320f1d1fe4c29814a1b635c1f))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * rinkaku-core bumped from 0.4.0 to 0.5.0

## [0.4.0](https://github.com/hiro-o918/rinkaku/compare/v0.3.0...v0.4.0) (2026-07-13)


### ⚠ BREAKING CHANGES

* any script or CI invocation of `rinkaku --include-tests` now fails to parse and must switch to omitting the flag (new default) or to `--exclude-tests` (previous default's exclusion). The tool has not shipped a stable CLI yet.

### Features

* add interactive TUI with directory-tree entry view ([#49](https://github.com/hiro-o918/rinkaku/issues/49)) ([29665e5](https://github.com/hiro-o918/rinkaku/commit/29665e5bf9e0395733088babc46bb5eebb56f566))
* default to including test symbols; rename --include-tests to --exclude-tests ([#69](https://github.com/hiro-o918/rinkaku/issues/69)) ([56a98b9](https://github.com/hiro-o918/rinkaku/commit/56a98b98c365902d8b8c355a1f1ea8ff5660248f))
* entry-path pivot — re-root the graph at a chosen path ([#56](https://github.com/hiro-o918/rinkaku/issues/56)) ([470f85b](https://github.com/hiro-o918/rinkaku/commit/470f85b99b242f93241ce5a27af4976629ad0b2f))
* **tui:** add diff pane and directory/file detail views ([#51](https://github.com/hiro-o918/rinkaku/issues/51)) ([23ae23b](https://github.com/hiro-o918/rinkaku/commit/23ae23b620950cb015451367b3012021293a0122))
* **tui:** add right-pane scrolling with an overflow indicator ([#54](https://github.com/hiro-o918/rinkaku/issues/54)) ([3dd002e](https://github.com/hiro-o918/rinkaku/commit/3dd002e0dc7259318d86e1f1a7c7d1270cb4d55f))
* **tui:** interaction model v2 — focus, default diff, scoped hunks, help overlay ([#61](https://github.com/hiro-o918/rinkaku/issues/61)) ([1edad34](https://github.com/hiro-o918/rinkaku/commit/1edad34c70c61b101c8763efb30a1e3438281f99))
* **tui:** jump to callers/callees with gd/gr and a jumplist ([#62](https://github.com/hiro-o918/rinkaku/issues/62)) ([c951555](https://github.com/hiro-o918/rinkaku/commit/c951555787d3e694d76229ec9036ce7f3774b678))
* **tui:** rename the pivot pane to blast radius ([#64](https://github.com/hiro-o918/rinkaku/issues/64)) ([f8b188f](https://github.com/hiro-o918/rinkaku/commit/f8b188f67a9db2ece2c552afa8001f7f3eaf0e29))
* **tui:** scrollable source screen and half-page/top-bottom keys on the right pane ([#70](https://github.com/hiro-o918/rinkaku/issues/70)) ([84ba727](https://github.com/hiro-o918/rinkaku/commit/84ba7273762bfd80d4ea83ab36fe13c91250e999))
* **tui:** show skipped and test-only files in the entry tree ([#58](https://github.com/hiro-o918/rinkaku/issues/58)) ([e4c21d3](https://github.com/hiro-o918/rinkaku/commit/e4c21d3b2f4fb8c461c63ad9b10c96fcb279c6da))
* **tui:** syntax-highlight the diff pane via tree-sitter ([#55](https://github.com/hiro-o918/rinkaku/issues/55)) ([b314e79](https://github.com/hiro-o918/rinkaku/commit/b314e799d4a8949ca458e591c47f43866331f7b7))
* **tui:** syntax-highlight the source drill-down ([#68](https://github.com/hiro-o918/rinkaku/issues/68)) ([bd88f5d](https://github.com/hiro-o918/rinkaku/commit/bd88f5da0293cdb29eaf08bc3cb36e4a64791a64))
* whole-repo outline as the default input mode ([#52](https://github.com/hiro-o918/rinkaku/issues/52)) ([b8f551a](https://github.com/hiro-o918/rinkaku/commit/b8f551a24c02684b880bfe7071f1073cb8509d38))


### Bug Fixes

* bump rinkaku-core dep to 0.2.0 in rinkaku and rinkaku-tui ([#72](https://github.com/hiro-o918/rinkaku/issues/72)) ([886cb05](https://github.com/hiro-o918/rinkaku/commit/886cb05a76af602e9fa81166e05a74884fa3e711))
* **tui:** open the TUI when the diff is piped via stdin ([#67](https://github.com/hiro-o918/rinkaku/issues/67)) ([62947ad](https://github.com/hiro-o918/rinkaku/commit/62947ade7a6130ec237458a8971bd089b2362abe))
* **tui:** resolve source view paths against the repository root ([#57](https://github.com/hiro-o918/rinkaku/issues/57)) ([0ff7c01](https://github.com/hiro-o918/rinkaku/commit/0ff7c01487844c66230ab3cfb50d1d4dcc4e245a))
* **tui:** unify Enter on the diff pane and fix scroll behavior ([#65](https://github.com/hiro-o918/rinkaku/issues/65)) ([0de6f81](https://github.com/hiro-o918/rinkaku/commit/0de6f81546a918d89a6e458d0c2664eb2cef545d))
* unify release-please into a single-PR release cycle ([#73](https://github.com/hiro-o918/rinkaku/issues/73)) ([796d249](https://github.com/hiro-o918/rinkaku/commit/796d249ee7067ede9207dcd54a643e028f0e123a))


### Miscellaneous

* release main ([#50](https://github.com/hiro-o918/rinkaku/issues/50)) ([8791d00](https://github.com/hiro-o918/rinkaku/commit/8791d00edeba0f84d7b028ce46a05e7f4ec0c178))
* release main ([#74](https://github.com/hiro-o918/rinkaku/issues/74)) ([5e0986f](https://github.com/hiro-o918/rinkaku/commit/5e0986f3048457ccee5283a3e7770fc149c7e4c7))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * rinkaku-core bumped from 0.3.0 to 0.4.0

## [0.3.0](https://github.com/hiro-o918/rinkaku/compare/v0.2.0...v0.3.0) (2026-07-13)


### ⚠ BREAKING CHANGES

* any script or CI invocation of `rinkaku --include-tests` now fails to parse and must switch to omitting the flag (new default) or to `--exclude-tests` (previous default's exclusion). The tool has not shipped a stable CLI yet.

### Features

* add interactive TUI with directory-tree entry view ([#49](https://github.com/hiro-o918/rinkaku/issues/49)) ([29665e5](https://github.com/hiro-o918/rinkaku/commit/29665e5bf9e0395733088babc46bb5eebb56f566))
* default to including test symbols; rename --include-tests to --exclude-tests ([#69](https://github.com/hiro-o918/rinkaku/issues/69)) ([56a98b9](https://github.com/hiro-o918/rinkaku/commit/56a98b98c365902d8b8c355a1f1ea8ff5660248f))
* entry-path pivot — re-root the graph at a chosen path ([#56](https://github.com/hiro-o918/rinkaku/issues/56)) ([470f85b](https://github.com/hiro-o918/rinkaku/commit/470f85b99b242f93241ce5a27af4976629ad0b2f))
* **tui:** add diff pane and directory/file detail views ([#51](https://github.com/hiro-o918/rinkaku/issues/51)) ([23ae23b](https://github.com/hiro-o918/rinkaku/commit/23ae23b620950cb015451367b3012021293a0122))
* **tui:** add right-pane scrolling with an overflow indicator ([#54](https://github.com/hiro-o918/rinkaku/issues/54)) ([3dd002e](https://github.com/hiro-o918/rinkaku/commit/3dd002e0dc7259318d86e1f1a7c7d1270cb4d55f))
* **tui:** interaction model v2 — focus, default diff, scoped hunks, help overlay ([#61](https://github.com/hiro-o918/rinkaku/issues/61)) ([1edad34](https://github.com/hiro-o918/rinkaku/commit/1edad34c70c61b101c8763efb30a1e3438281f99))
* **tui:** jump to callers/callees with gd/gr and a jumplist ([#62](https://github.com/hiro-o918/rinkaku/issues/62)) ([c951555](https://github.com/hiro-o918/rinkaku/commit/c951555787d3e694d76229ec9036ce7f3774b678))
* **tui:** rename the pivot pane to blast radius ([#64](https://github.com/hiro-o918/rinkaku/issues/64)) ([f8b188f](https://github.com/hiro-o918/rinkaku/commit/f8b188f67a9db2ece2c552afa8001f7f3eaf0e29))
* **tui:** scrollable source screen and half-page/top-bottom keys on the right pane ([#70](https://github.com/hiro-o918/rinkaku/issues/70)) ([84ba727](https://github.com/hiro-o918/rinkaku/commit/84ba7273762bfd80d4ea83ab36fe13c91250e999))
* **tui:** show skipped and test-only files in the entry tree ([#58](https://github.com/hiro-o918/rinkaku/issues/58)) ([e4c21d3](https://github.com/hiro-o918/rinkaku/commit/e4c21d3b2f4fb8c461c63ad9b10c96fcb279c6da))
* **tui:** syntax-highlight the diff pane via tree-sitter ([#55](https://github.com/hiro-o918/rinkaku/issues/55)) ([b314e79](https://github.com/hiro-o918/rinkaku/commit/b314e799d4a8949ca458e591c47f43866331f7b7))
* **tui:** syntax-highlight the source drill-down ([#68](https://github.com/hiro-o918/rinkaku/issues/68)) ([bd88f5d](https://github.com/hiro-o918/rinkaku/commit/bd88f5da0293cdb29eaf08bc3cb36e4a64791a64))
* whole-repo outline as the default input mode ([#52](https://github.com/hiro-o918/rinkaku/issues/52)) ([b8f551a](https://github.com/hiro-o918/rinkaku/commit/b8f551a24c02684b880bfe7071f1073cb8509d38))


### Bug Fixes

* bump rinkaku-core dep to 0.2.0 in rinkaku and rinkaku-tui ([#72](https://github.com/hiro-o918/rinkaku/issues/72)) ([886cb05](https://github.com/hiro-o918/rinkaku/commit/886cb05a76af602e9fa81166e05a74884fa3e711))
* **tui:** open the TUI when the diff is piped via stdin ([#67](https://github.com/hiro-o918/rinkaku/issues/67)) ([62947ad](https://github.com/hiro-o918/rinkaku/commit/62947ade7a6130ec237458a8971bd089b2362abe))
* **tui:** resolve source view paths against the repository root ([#57](https://github.com/hiro-o918/rinkaku/issues/57)) ([0ff7c01](https://github.com/hiro-o918/rinkaku/commit/0ff7c01487844c66230ab3cfb50d1d4dcc4e245a))
* **tui:** unify Enter on the diff pane and fix scroll behavior ([#65](https://github.com/hiro-o918/rinkaku/issues/65)) ([0de6f81](https://github.com/hiro-o918/rinkaku/commit/0de6f81546a918d89a6e458d0c2664eb2cef545d))
* unify release-please into a single-PR release cycle ([#73](https://github.com/hiro-o918/rinkaku/issues/73)) ([796d249](https://github.com/hiro-o918/rinkaku/commit/796d249ee7067ede9207dcd54a643e028f0e123a))


### Miscellaneous

* release main ([#50](https://github.com/hiro-o918/rinkaku/issues/50)) ([8791d00](https://github.com/hiro-o918/rinkaku/commit/8791d00edeba0f84d7b028ce46a05e7f4ec0c178))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * rinkaku-core bumped from 0.2.1 to 0.3.0

## [0.2.0](https://github.com/hiro-o918/rinkaku/compare/rinkaku-tui-v0.1.0...rinkaku-tui-v0.2.0) (2026-07-13)


### ⚠ BREAKING CHANGES

* any script or CI invocation of `rinkaku --include-tests` now fails to parse and must switch to omitting the flag (new default) or to `--exclude-tests` (previous default's exclusion). The tool has not shipped a stable CLI yet.

### Features

* add interactive TUI with directory-tree entry view ([#49](https://github.com/hiro-o918/rinkaku/issues/49)) ([29665e5](https://github.com/hiro-o918/rinkaku/commit/29665e5bf9e0395733088babc46bb5eebb56f566))
* default to including test symbols; rename --include-tests to --exclude-tests ([#69](https://github.com/hiro-o918/rinkaku/issues/69)) ([56a98b9](https://github.com/hiro-o918/rinkaku/commit/56a98b98c365902d8b8c355a1f1ea8ff5660248f))
* entry-path pivot — re-root the graph at a chosen path ([#56](https://github.com/hiro-o918/rinkaku/issues/56)) ([470f85b](https://github.com/hiro-o918/rinkaku/commit/470f85b99b242f93241ce5a27af4976629ad0b2f))
* **tui:** add diff pane and directory/file detail views ([#51](https://github.com/hiro-o918/rinkaku/issues/51)) ([23ae23b](https://github.com/hiro-o918/rinkaku/commit/23ae23b620950cb015451367b3012021293a0122))
* **tui:** add right-pane scrolling with an overflow indicator ([#54](https://github.com/hiro-o918/rinkaku/issues/54)) ([3dd002e](https://github.com/hiro-o918/rinkaku/commit/3dd002e0dc7259318d86e1f1a7c7d1270cb4d55f))
* **tui:** interaction model v2 — focus, default diff, scoped hunks, help overlay ([#61](https://github.com/hiro-o918/rinkaku/issues/61)) ([1edad34](https://github.com/hiro-o918/rinkaku/commit/1edad34c70c61b101c8763efb30a1e3438281f99))
* **tui:** jump to callers/callees with gd/gr and a jumplist ([#62](https://github.com/hiro-o918/rinkaku/issues/62)) ([c951555](https://github.com/hiro-o918/rinkaku/commit/c951555787d3e694d76229ec9036ce7f3774b678))
* **tui:** rename the pivot pane to blast radius ([#64](https://github.com/hiro-o918/rinkaku/issues/64)) ([f8b188f](https://github.com/hiro-o918/rinkaku/commit/f8b188f67a9db2ece2c552afa8001f7f3eaf0e29))
* **tui:** scrollable source screen and half-page/top-bottom keys on the right pane ([#70](https://github.com/hiro-o918/rinkaku/issues/70)) ([84ba727](https://github.com/hiro-o918/rinkaku/commit/84ba7273762bfd80d4ea83ab36fe13c91250e999))
* **tui:** show skipped and test-only files in the entry tree ([#58](https://github.com/hiro-o918/rinkaku/issues/58)) ([e4c21d3](https://github.com/hiro-o918/rinkaku/commit/e4c21d3b2f4fb8c461c63ad9b10c96fcb279c6da))
* **tui:** syntax-highlight the diff pane via tree-sitter ([#55](https://github.com/hiro-o918/rinkaku/issues/55)) ([b314e79](https://github.com/hiro-o918/rinkaku/commit/b314e799d4a8949ca458e591c47f43866331f7b7))
* **tui:** syntax-highlight the source drill-down ([#68](https://github.com/hiro-o918/rinkaku/issues/68)) ([bd88f5d](https://github.com/hiro-o918/rinkaku/commit/bd88f5da0293cdb29eaf08bc3cb36e4a64791a64))
* whole-repo outline as the default input mode ([#52](https://github.com/hiro-o918/rinkaku/issues/52)) ([b8f551a](https://github.com/hiro-o918/rinkaku/commit/b8f551a24c02684b880bfe7071f1073cb8509d38))


### Bug Fixes

* bump rinkaku-core dep to 0.2.0 in rinkaku and rinkaku-tui ([#72](https://github.com/hiro-o918/rinkaku/issues/72)) ([886cb05](https://github.com/hiro-o918/rinkaku/commit/886cb05a76af602e9fa81166e05a74884fa3e711))
* **tui:** open the TUI when the diff is piped via stdin ([#67](https://github.com/hiro-o918/rinkaku/issues/67)) ([62947ad](https://github.com/hiro-o918/rinkaku/commit/62947ade7a6130ec237458a8971bd089b2362abe))
* **tui:** resolve source view paths against the repository root ([#57](https://github.com/hiro-o918/rinkaku/issues/57)) ([0ff7c01](https://github.com/hiro-o918/rinkaku/commit/0ff7c01487844c66230ab3cfb50d1d4dcc4e245a))
* **tui:** unify Enter on the diff pane and fix scroll behavior ([#65](https://github.com/hiro-o918/rinkaku/issues/65)) ([0de6f81](https://github.com/hiro-o918/rinkaku/commit/0de6f81546a918d89a6e458d0c2664eb2cef545d))
* unify release-please into a single-PR release cycle ([#73](https://github.com/hiro-o918/rinkaku/issues/73)) ([796d249](https://github.com/hiro-o918/rinkaku/commit/796d249ee7067ede9207dcd54a643e028f0e123a))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * rinkaku-core bumped from 0.2.0 to 0.2.1
