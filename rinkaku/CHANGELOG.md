# Changelog

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
