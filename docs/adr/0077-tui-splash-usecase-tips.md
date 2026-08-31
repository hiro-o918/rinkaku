# 0077. A fixed usecase tip on the splash screen

- Status: Accepted
- Date: 2026-08-31

## Context

ADR 0033's splash screen keeps the reviewer informed of *what phase* the
pipeline is in, but most phases (`ResolvingPr`, `Diffing`) are label-only
— no gauge, since there is no real per-file signal to show (ADR 0033
decision 3's "no fake progress" stance). On a slow `--pr`/`--base` run
against a large or remote repository, the screen can sit on one label for
several seconds with nothing else to look at.

Separately, this TUI's discoverability is uneven: `?` surfaces the full
keymap, but only once the reviewer is already inside a session and thinks
to press it. Several genuinely useful capabilities — stacked-PR layer
switching (`gt`/`gT`, ADR 0075/0076), the `--format json`/`--base main`
combination for handing a change map to an LLM, `--entry` re-rooting —
have no prompt at all pointing a first-time or infrequent reviewer toward
them.

## Decision

**Show exactly one usecase tip per run, fixed for the whole splash
duration.** `rinkaku-tui` gains a `tips` module: `pick_tip(locale: Locale,
seed: u64) -> String` selects one of a fixed set of tip keys via `seed %
len` and resolves it through the same `rust_i18n::t!` mechanism ADR 0055
already established for the help overlay. The function is pure — no clock,
no RNG — matching this crate's IO-at-the-boundary rule; the seed is
derived once by `rinkaku`'s `main.rs` composition root (`SystemTime::now()`
as seconds since the epoch) and passed in.

**The tip does not rotate.** A tip that changed every few seconds while
the splash screen sits on one phase would itself read as simulated
motion — exactly what ADR 0033 decision 3 already rejected for progress
bars ("a bar that doesn't correspond to real work either lies about how
much is left or, worse, stalls visibly and looks broken"). A static tip
carries no such claim; a rotating one would imply progress that isn't
happening. `SplashState` gains `tip: Option<String>`, computed once and
threaded through every phase/progress redraw for the run — the same
value on every call, never recomputed per phase.

**Scope amendment to ADR 0055.** That ADR scoped `rinkaku-tui`
localization to the `?` help overlay's prose specifically, reasoning that
every other screen is short labels a non-native English reader working in
this codebase daily is already comfortable with. The splash tip is the
one exception: it is prose meant to teach a capability, read once at
startup by someone who may not yet be comfortable with the tool's
vocabulary at all (unlike the help overlay, read by someone already
mid-session). Both `en`/`ja` tip text live under a `tips:` key in the same
`rinkaku-tui/locales/{en,ja}.yml` pair `help:` already uses.

**Tip content is usecase-shaped**, not keybinding fragments: each tip
states a situation ("reviewing a stacked PR", "handing this to an LLM")
and the one flag/keybinding combination that addresses it, verified
against `rinkaku/src/cli.rs`'s actual flag names and `rinkaku-tui/src/help.rs`'s
actual keymap rather than invented shorthand.

**Rendering**: `draw_splash` shows the tip one blank line below the phase
label (and gauge, when present), dim, wrapped, and centered at the same
modest width as the gauge. On a terminal too short to fit it, the tip is
omitted entirely rather than truncated — the same "don't show a degraded
form" precedent the gauge already sets by only drawing when `Some`.

## Alternatives

- **Rotate through several tips during one run.** Rejected: see Decision
  above — this is the "no fake progress" principle applied to a second
  kind of splash content, not just the gauge.
- **Show the tip on every run, unconditionally, without varying it.**
  Rejected: a single always-shown tip stops being read after the first
  few runs; varying it per run (deterministically, not per redraw) keeps
  it worth glancing at without introducing motion.
- **True randomness (a `rand` dependency) for tip selection.** Rejected:
  the tip only needs to vary run to run, not be unpredictable or
  uniformly distributed under adversarial conditions — `SystemTime::now()`
  seconds is sufficient and adds no new dependency.
- **Localize every screen**, revisiting ADR 0055's scope wholesale.
  Rejected: ADR 0055's reasoning for the other screens (short, code-
  adjacent labels a daily user is comfortable with) still holds; only the
  splash tip's prose-and-first-run-audience combination changes the
  calculus, so the amendment is scoped to that one addition, not a
  reopening of the whole decision.

## Consequences

- **`rinkaku-tui` API**: adds `pub mod tips` (`pick_tip`). `SplashState`
  gains a `tip: Option<String>` field and a `with_tip` builder;
  `label_only`/`with_progress` are unchanged in signature, defaulting
  `tip` to `None`.
- **Localization scope** (ADR 0055 amendment): `rinkaku-tui/locales/{en,ja}.yml`
  gain a `tips:` section, translated the same way `help:` already is.
  Every other screen's English-only scope is unchanged.
- **`rinkaku` bin**: `main.rs`'s composition root gains a `splash_tip_seed`
  helper (`SystemTime::now()`) and calls `pick_tip` once, before
  `TuiSession::init`, threading the resulting `String` through the
  bootstrap `draw_splash` call and into `SplashProgress::new` so every
  subsequent phase/progress redraw during the pre-render pipeline carries
  the same tip.
- **Testing**: `pick_tip` is unit tested as a pure function (`rstest` +
  `pretty_assertions`) — deterministic per seed, and resolving to a real
  translation (not an echoed-back missing key) across a swept range of
  seeds in both locales. `draw_splash`'s tip rendering gets the same
  coarse `TestBackend` treatment the gauge already has: shown when there's
  room, omitted when there isn't.
