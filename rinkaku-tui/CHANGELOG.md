# Changelog

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
