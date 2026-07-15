//! The `?` help overlay's locale (ADR 0055): which language its prose
//! renders in. Detection follows POSIX's `LC_ALL > LC_MESSAGES > LANG`
//! precedence, but only as far as this project's actual need — a binary
//! choice between English and Japanese — so [`detect_locale`] takes
//! already-read env values and returns a [`Locale`], with the real
//! `std::env::var` reads left to `rinkaku`'s `main.rs` composition root
//! (this crate's own pure/IO split, `lib.rs`'s module doc comment).

/// The `?` help overlay's rendering language. Every other screen in this
/// crate stays English-only (ADR 0055's scope decision), so this only
/// governs [`crate::help::help_content`] and its call sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    English,
    Japanese,
}

impl Locale {
    /// The `rust-i18n` locale tag this variant maps to.
    pub fn tag(self) -> &'static str {
        match self {
            Locale::English => "en",
            Locale::Japanese => "ja",
        }
    }
}

/// Picks [`Locale::Japanese`] when the first `Some` value among
/// `lc_all`, `lc_messages`, `lang` (POSIX precedence) has a `ja` language
/// prefix (the substring before the first `.`, `_`, or `@`), else
/// [`Locale::English`] — including when all three are `None`.
pub fn detect_locale(
    lc_all: Option<&str>,
    lc_messages: Option<&str>,
    lang: Option<&str>,
) -> Locale {
    let chosen = lc_all.or(lc_messages).or(lang);
    match chosen {
        Some(value) if language_prefix(value) == "ja" => Locale::Japanese,
        _ => Locale::English,
    }
}

fn language_prefix(value: &str) -> &str {
    let end = value.find(['.', '_', '@']).unwrap_or(value.len());
    &value[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case::should_select_english_when_all_env_vars_are_unset(None, None, None, Locale::English)]
    #[case::should_select_japanese_when_lang_is_bare_ja(None, None, Some("ja"), Locale::Japanese)]
    #[case::should_select_japanese_when_lang_is_ja_jp_utf8(
        None,
        None,
        Some("ja_JP.UTF-8"),
        Locale::Japanese
    )]
    #[case::should_select_english_when_lang_is_en_us_utf8(
        None,
        None,
        Some("en_US.UTF-8"),
        Locale::English
    )]
    #[case::should_select_english_when_lang_is_unrelated_language(
        None,
        None,
        Some("fr_FR.UTF-8"),
        Locale::English
    )]
    #[case::should_prefer_lc_all_over_lc_messages_and_lang(
        Some("ja_JP.UTF-8"),
        Some("en_US.UTF-8"),
        Some("en_US.UTF-8"),
        Locale::Japanese
    )]
    #[case::should_prefer_lc_messages_over_lang_when_lc_all_is_unset(
        None,
        Some("ja_JP.UTF-8"),
        Some("en_US.UTF-8"),
        Locale::Japanese
    )]
    #[case::should_fall_back_to_lang_when_lc_all_and_lc_messages_are_unset(
        None,
        None,
        Some("ja_JP.UTF-8"),
        Locale::Japanese
    )]
    #[case::should_select_english_when_lc_all_is_set_to_c(
        Some("C"),
        None,
        Some("ja_JP.UTF-8"),
        Locale::English
    )]
    #[case::should_select_japanese_when_lang_has_at_modifier(
        None,
        None,
        Some("ja@somemodifier"),
        Locale::Japanese
    )]
    fn should_detect_locale_per_posix_precedence(
        #[case] lc_all: Option<&str>,
        #[case] lc_messages: Option<&str>,
        #[case] lang: Option<&str>,
        #[case] expected: Locale,
    ) {
        let actual = detect_locale(lc_all, lc_messages, lang);
        assert_eq!(expected, actual);
    }

    #[rstest]
    #[case::english_tag(Locale::English, "en")]
    #[case::japanese_tag(Locale::Japanese, "ja")]
    fn should_map_locale_to_rust_i18n_tag(#[case] locale: Locale, #[case] expected: &str) {
        let actual = locale.tag();
        assert_eq!(expected, actual);
    }
}
