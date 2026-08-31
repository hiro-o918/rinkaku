//! One fixed usecase tip shown on the splash screen (ADR 0077), picked
//! once per run and held for its whole duration — never rotated, to stay
//! consistent with ADR 0033 decision 3's "no fake progress" stance: a tip
//! that changed every few seconds would itself read as simulated motion.
//!
//! [`pick_tip`] is pure (no clock, no RNG): the composition root
//! (`rinkaku`'s `main.rs`) derives `seed` once from `SystemTime::now()`
//! and passes it in, keeping the same IO-at-the-boundary split this
//! crate's other locale-aware content (`crate::help`) already follows.

use crate::locale::Locale;

/// `rust_i18n` key suffixes under `tips.*` in both locale YAML files —
/// order fixes each key's index, which [`pick_tip`]'s `seed % len` uses
/// to select one.
const TIP_KEYS: &[&str] = &[
    "stacked_pr",
    "llm_export",
    "reading_order",
    "diff_pane_sync",
    "entry_blast_radius",
    "review_notes",
    "discoverability",
];

/// Selects one usecase tip for `locale`, deterministically from `seed`
/// (`seed % TIP_KEYS.len()`).
pub fn pick_tip(locale: Locale, seed: u64) -> String {
    let index = (seed % TIP_KEYS.len() as u64) as usize;
    let key = TIP_KEYS[index];
    rust_i18n::t!(format!("tips.{key}"), locale = locale.tag()).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case::english(Locale::English)]
    #[case::japanese(Locale::Japanese)]
    fn should_return_same_tip_when_seed_is_repeated(#[case] locale: Locale) {
        let first = pick_tip(locale, 42);
        let second = pick_tip(locale, 42);

        assert_eq!(first, second);
    }

    #[rstest]
    #[case::english(Locale::English)]
    #[case::japanese(Locale::Japanese)]
    fn should_resolve_a_real_translation_for_every_seed_in_both_locales(#[case] locale: Locale) {
        for seed in 0..(TIP_KEYS.len() as u64 * 3) {
            let tip = pick_tip(locale, seed);
            let key = TIP_KEYS[(seed % TIP_KEYS.len() as u64) as usize];

            // `rust_i18n::t!` echoes the lookup key back verbatim when a
            // translation is missing, rather than erroring — asserting
            // against that exact fallback string is what actually catches
            // a YAML key typo; a bare "is non-empty" check would not.
            assert_ne!(format!("tips.{key}"), tip);
        }
    }
}
