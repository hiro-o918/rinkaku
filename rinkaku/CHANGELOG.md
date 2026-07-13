# Changelog

## [0.5.1](https://github.com/hiro-o918/rinkaku/compare/v0.5.0...v0.5.1) (2026-07-13)


### Bug Fixes

* unify release-please into a single-PR release cycle ([#73](https://github.com/hiro-o918/rinkaku/issues/73)) ([796d249](https://github.com/hiro-o918/rinkaku/commit/796d249ee7067ede9207dcd54a643e028f0e123a))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * rinkaku-core bumped from 0.2.0 to 0.2.1
    * rinkaku-tui bumped from 0.1.0 to 0.2.0

## [0.5.0](https://github.com/hiro-o918/rinkaku/compare/v0.4.1...v0.5.0) (2026-07-13)


### ⚠ BREAKING CHANGES

* any script or CI invocation of `rinkaku --include-tests` now fails to parse and must switch to omitting the flag (new default) or to `--exclude-tests` (previous default's exclusion). The tool has not shipped a stable CLI yet.

### Features

* add interactive TUI with directory-tree entry view ([#49](https://github.com/hiro-o918/rinkaku/issues/49)) ([29665e5](https://github.com/hiro-o918/rinkaku/commit/29665e5bf9e0395733088babc46bb5eebb56f566))
* classify changed symbols by contract impact ([#45](https://github.com/hiro-o918/rinkaku/issues/45)) ([b8a8b37](https://github.com/hiro-o918/rinkaku/commit/b8a8b37ab6af44fae5e6bdc2db0891f3d7f545d0))
* default to including test symbols; rename --include-tests to --exclude-tests ([#69](https://github.com/hiro-o918/rinkaku/issues/69)) ([56a98b9](https://github.com/hiro-o918/rinkaku/commit/56a98b98c365902d8b8c355a1f1ea8ff5660248f))
* detect generated files by content markers and drop them from output ([#39](https://github.com/hiro-o918/rinkaku/issues/39)) ([506f62e](https://github.com/hiro-o918/rinkaku/commit/506f62ef8565401a56c2c464a9b696e86aed2e0e))
* entry-path pivot — re-root the graph at a chosen path ([#56](https://github.com/hiro-o918/rinkaku/issues/56)) ([470f85b](https://github.com/hiro-o918/rinkaku/commit/470f85b99b242f93241ce5a27af4976629ad0b2f))
* exclude test symbols and generated files from output by default ([#38](https://github.com/hiro-o918/rinkaku/issues/38)) ([44d4e3b](https://github.com/hiro-o918/rinkaku/commit/44d4e3bee9e96bd0e366fe7fd75ce41d87e6f898))
* mermaid output format and PR report GitHub Action ([#59](https://github.com/hiro-o918/rinkaku/issues/59)) ([b706067](https://github.com/hiro-o918/rinkaku/commit/b70606719d1427d8cb69ff007ab2b43251528ad2))
* render output as entry-point trees over the changed-symbol graph ([#35](https://github.com/hiro-o918/rinkaku/issues/35)) ([1b53bdc](https://github.com/hiro-o918/rinkaku/commit/1b53bdc4e72d99fe45703cfd989aba1b9e021082))
* surface fan-in hotspots in rendered output ([#43](https://github.com/hiro-o918/rinkaku/issues/43)) ([b38cf86](https://github.com/hiro-o918/rinkaku/commit/b38cf867fd197e897229bd580d31708457368c59))
* **tui:** add diff pane and directory/file detail views ([#51](https://github.com/hiro-o918/rinkaku/issues/51)) ([23ae23b](https://github.com/hiro-o918/rinkaku/commit/23ae23b620950cb015451367b3012021293a0122))
* **tui:** rename the pivot pane to blast radius ([#64](https://github.com/hiro-o918/rinkaku/issues/64)) ([f8b188f](https://github.com/hiro-o918/rinkaku/commit/f8b188f67a9db2ece2c552afa8001f7f3eaf0e29))
* whole-repo outline as the default input mode ([#52](https://github.com/hiro-o918/rinkaku/issues/52)) ([b8f551a](https://github.com/hiro-o918/rinkaku/commit/b8f551a24c02684b880bfe7071f1073cb8509d38))


### Bug Fixes

* bump rinkaku-core dep to 0.2.0 in rinkaku and rinkaku-tui ([#72](https://github.com/hiro-o918/rinkaku/issues/72)) ([886cb05](https://github.com/hiro-o918/rinkaku/commit/886cb05a76af602e9fa81166e05a74884fa3e711))
* surface git stderr when cat-file batch write fails ([#41](https://github.com/hiro-o918/rinkaku/issues/41)) ([2b0bd31](https://github.com/hiro-o918/rinkaku/commit/2b0bd318183c2b1ad666ec442b394792f2a70b10))
* **tui:** resolve source view paths against the repository root ([#57](https://github.com/hiro-o918/rinkaku/issues/57)) ([0ff7c01](https://github.com/hiro-o918/rinkaku/commit/0ff7c01487844c66230ab3cfb50d1d4dcc4e245a))

## [0.4.1](https://github.com/hiro-o918/rinkaku/compare/v0.4.0...v0.4.1) (2026-07-12)


### Features

* log progress to stderr by default ([aa7ca34](https://github.com/hiro-o918/rinkaku/commit/aa7ca3418651ca1fecbe413adf9354ec698217fa))


### Bug Fixes

* base --pr diffs on baseRefOid so merged PRs produce output ([73eb482](https://github.com/hiro-o918/rinkaku/commit/73eb482bda4c11b5f0f5ea60b10b6e9f8f00d5f1))
* make resolve_pr_base_sha's base-branch fetch failure soft and avoid a redundant refetch ([0149768](https://github.com/hiro-o918/rinkaku/commit/014976889bd80fd37fa6bd05f736372508181e54))
* own self-update messaging instead of the crate's output ([7e423ea](https://github.com/hiro-o918/rinkaku/commit/7e423eab9d54b8252d01b512274aea85ffca2b06))
* report empty diffs and skip dependency indexing when the diff is empty ([af9fbf8](https://github.com/hiro-o918/rinkaku/commit/af9fbf85aeaa574584b942bad2c1adb204db493b))
* restore per-file isolation and drain stderr in the cat-file batch reader ([001d8c9](https://github.com/hiro-o918/rinkaku/commit/001d8c99e3f21b6fef82ea1769a1ef5805d2f8f0))

## [0.4.0](https://github.com/hiro-o918/rinkaku/compare/v0.3.1...v0.4.0) (2026-07-12)


### Features

* auto-clone PR URL repos into a cache when outside a clone ([47993ce](https://github.com/hiro-o918/rinkaku/commit/47993ce3764949a567f1116bfc13db2cdb8d3072))
* prefer existing ghq-managed clones over the cache clone ([faaf230](https://github.com/hiro-o918/rinkaku/commit/faaf2305a7d53a1538b444ab926b049450f10599))

## [0.3.1](https://github.com/hiro-o918/rinkaku/compare/v0.3.0...v0.3.1) (2026-07-12)


### Documentation

* cross-reference README's Release section from self_update.rs ([fee4fce](https://github.com/hiro-o918/rinkaku/commit/fee4fce8cba488f1efbef34a34b35eddb2d590a2))

## [0.3.0](https://github.com/hiro-o918/rinkaku/compare/v0.2.0...v0.3.0) (2026-07-12)


### Features

* add --pr input mode resolving GitHub PRs via gh ([5fb0464](https://github.com/hiro-o918/rinkaku/commit/5fb0464c5a0391894540c4fdc5ca853253c3f946))

## [0.2.0](https://github.com/hiro-o918/rinkaku/compare/v0.1.0...v0.2.0) (2026-07-12)


### Features

* add self-update subcommand ([a16aeda](https://github.com/hiro-o918/rinkaku/commit/a16aeda1cdc256565383816f224a2a917f13a599))


### Bug Fixes

* refuse non-interactive self-update and add --yes flag ([8896bf6](https://github.com/hiro-o918/rinkaku/commit/8896bf64e2b839053663d80c96e36d650048e5d6))

## 0.1.0 (2026-07-12)


### Bug Fixes

* declare explicit crate versions for release-please compatibility ([6002680](https://github.com/hiro-o918/rinkaku/commit/600268047ae2f36dd7bd0ad96f3a1d7fda4fc9ab))
