# 0055. TUI: Japanese localization for the `?` help overlay

- Status: Accepted
- Date: 2026-07-15

## Context

The `?` help overlay (`rinkaku-tui/src/help.rs`, ADR 0020) is the one part
of this TUI that carries long, descriptive prose rather than short labels:
key-binding descriptions, marker-legend explanations, and a glossary of
terms like "blast radius" and "topological order". Everywhere else in the
TUI — the status bar, the Entry/Diff panes, error messages — is made of
short labels and code-adjacent text that a non-native English reader who
works in this codebase daily is already comfortable with. The overlay's
prose is the one place where reading speed in a second language actually
matters, since it exists specifically to be read by someone who forgot
what a keybinding does.

Non-English rinkaku users are engineers who already read English
identifiers, error messages, and this project's own source comments
without friction. Translating the whole TUI would trade a consistent
vocabulary (English technical terms throughout) for a mixed one, for no
real gain outside the one screen that is actually prose-heavy. This ADR
therefore scopes localization to the help overlay's content only:
`KeyBinding.description`, `GlossaryEntry.explanation`,
`MarkerLegendEntry.explanation`, and the overlay's section headings.
Key labels (`j/k`), glossary term names, and every other screen stay
English-only.

## Decision

**Adopt `rust-i18n`** for the translated strings, with
`rinkaku-tui/locales/en.yml` and `rinkaku-tui/locales/ja.yml` compiled in
at build time (`rust_i18n::i18n!` embeds the YAML via a proc macro, no
runtime file IO). English is the fixed default: every existing test
asserting on the overlay's English text must keep passing byte-for-byte,
so English is not merely "the fallback locale" but the locale those tests
pin.

**No global `rust_i18n::set_locale()`.** This crate's own module doc
comment (`lib.rs`) already draws a hard line between the pure view-model
layer and the terminal adapter that performs IO; a process-wide mutable
locale global would let any call site silently depend on ambient state
set somewhere else, which is exactly the kind of hidden coupling that
line exists to prevent. Instead, every translated lookup is an explicit
`t!("help.key", locale = ...)` call, and a `Locale` value (`English` or
`Japanese`) is threaded as a plain parameter from the boundary down to
`draw_help_overlay`.

**`Locale` is threaded as a draw-time parameter, not as `App` state.**
`App` is this crate's pure, unit-tested state machine, constructed at
over 300 call sites across the test suite (`App::new(report)` and its
`with_*` builders). Locale is fixed for the lifetime of a session and
never changes in response to a key press, so it has none of the
characteristics that justify living on `App` (no transition touches it,
no test needs to vary it per-`App`-instance). It is instead added as a
parameter to `rinkaku_tui::run`, `TuiSession::run`, `run_app`, and
`ui::draw` — the same chain `update_check` (ADR 0054) already travels —
terminating at `draw_help_overlay`, the only call site that reads it.
This keeps the change local to the render path and leaves `App`'s huge
existing call-site surface untouched.

**`help_content(locale: Locale) -> HelpContent` replaces the `const
HELP_CONTENT`.** `t!` allocates a `String`/`Cow` and cannot run in const
context, so the keymap/marker/glossary tables become a function
parameterized by locale, called once per draw the same way
`help_overlay_lines` already recomputes its `Vec<Line>` per draw. The
struct shapes (`KeyBinding`, `GlossaryEntry`, `MarkerLegendEntry`,
`KeyBindingGroup`, `HelpContent`) are unchanged; only their string fields
move from `&'static str` to `String`; `HELP_CONTENT` is unchanged.

**Locale detection is a pure function, tested in isolation from env
reads.** POSIX precedence — `LC_ALL` overrides `LC_MESSAGES` overrides
`LANG` — is well established outside this project; this decision does not
reinvent it, only implements the same precedence for one binary choice
(English or Japanese) instead of full POSIX locale parsing. The function
takes three `Option<&str>` values (already-read env vars) and returns
`Locale`: the first `Some` value in `LC_ALL, LC_MESSAGES, LANG` order
whose language prefix (the substring before the first `.`, `_`, or `@`)
is `ja` selects `Locale::Japanese`; every other case — including all
three unset, or a first-`Some` value with a non-`ja` prefix — selects
`Locale::English`. The actual `std::env::var(...)` reads happen only at
`rinkaku`'s `main.rs` composition root, immediately before constructing
the `TuiSession::run` call, mirroring how every other piece of IO in this
project is isolated to the boundary (`CLAUDE.md`'s "Core logic is pure"
principle, and ADR 0054's own `RINKAKU_UPDATE_CHECK` env read at the same
composition root). `rinkaku-core` is untouched by this ADR entirely.

## Alternatives

- **Hand-written `match`/lookup table** (a `fn translate(locale, key) ->
  &str` matching on an enum of message keys) instead of `rust-i18n`.
  Rejected: this project already treats `help.rs`'s content as a
  reviewer-facing table kept in sync by hand (`help.rs`'s own module doc
  comment); a hand-rolled lookup would duplicate what `rust-i18n`'s YAML
  + compile-time-checked key macro already gives for free, for a problem
  (two locales, ~30 short strings) far too small to justify inventing a
  second translation mechanism later if a third locale is ever added.
- **`fluent`** (Mozilla's ICU-message-format localization system).
  Rejected as disproportionate: `fluent` is built for plural rules,
  gendered forms, and complex ICU message syntax, none of which this
  overlay's short, plural-free English/Japanese strings need. `rust-i18n`
  covers the actual requirement (locale-keyed string lookup, compiled in)
  with a much smaller dependency and API surface.
- **Global `rust_i18n::set_locale()` at startup.** Rejected: see Decision
  above — this crate's pure/IO split already forbids ambient mutable
  state reaching the view-model layer, and a set-once-at-startup global
  is exactly that, even though it happens to be immutable in practice
  for the lifetime of one TUI session.
- **Full POSIX locale parsing** (respecting every LC_* category, charset
  suffixes, locale aliases). Rejected as scope well beyond this ADR's
  need: the only decision this project has to make is "English or
  Japanese for one overlay's prose", so matching the standard
  `LC_ALL > LC_MESSAGES > LANG` category precedence and a `ja` language-
  prefix check is sufficient; a general POSIX locale parser would be
  speculative infrastructure for locales this project does not yet
  support anywhere.
- **Translate every screen**, not just the help overlay. Rejected per
  Context above: the target audience is comfortable with English labels
  and short UI text; the actual friction is long prose, which only the
  help overlay has.

## Consequences

- `rinkaku-tui/Cargo.toml` gains a `rust-i18n` dependency and a
  `rinkaku-tui/locales/{en,ja}.yml` pair; `lib.rs` gains the crate-level
  `rust_i18n::i18n!` invocation.
- `rinkaku-tui/src/help.rs`'s `pub const HELP_CONTENT: HelpContent`
  becomes `pub fn help_content(locale: Locale) -> HelpContent`, and its
  struct fields move from `&'static str` to `String`; every existing call
  site that read `HELP_CONTENT` now calls `help_content(locale)` with a
  `Locale` value threaded in from its own caller.
- `rinkaku_tui::run`, `TuiSession::run`, `run_app`, and `ui::draw` each
  gain one new `Locale` parameter, threaded straight through to
  `draw_help_overlay` — the same shape ADR 0054's `update_check`
  parameter already added to the same call chain.
- `rinkaku/src/main.rs`'s composition root gains the three
  `std::env::var` reads (`LC_ALL`, `LC_MESSAGES`, `LANG`) and a call to
  the new pure detection function, immediately before the existing
  `session.run(...)` call.
- A future third locale, if ever needed, adds one more `locales/<lc>.yml`
  file and one more `Locale` variant — the detection function's
  `ja`-prefix check generalizes to a small match, not a redesign.
