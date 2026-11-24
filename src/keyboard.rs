use std::process::Command;
use std::sync::OnceLock;

use crate::spelling::DEFAULT_DICTIONARY;

enum LayoutLanguage {
    Dictionary(&'static str),
    /// Layouts like US or ABC are used for every English variant, so the
    /// system locale decides between US and UK spelling.
    AnyEnglish,
}

/// Matched against the layout name lowercased with punctuation removed;
/// the longest matching prefix wins, so `canadiancsa` beats `canadian`.
const LAYOUT_PREFIXES: [(&str, LayoutLanguage); 19] = [
    ("russian", LayoutLanguage::Dictionary("ru_RU")),
    ("german", LayoutLanguage::Dictionary("de_DE")),
    ("austrian", LayoutLanguage::Dictionary("de_DE")),
    ("swissgerman", LayoutLanguage::Dictionary("de_DE")),
    ("french", LayoutLanguage::Dictionary("fr_FR")),
    ("swissfrench", LayoutLanguage::Dictionary("fr_FR")),
    ("belgian", LayoutLanguage::Dictionary("fr_FR")),
    ("canadiancsa", LayoutLanguage::Dictionary("fr_FR")),
    ("canadianfrench", LayoutLanguage::Dictionary("fr_FR")),
    ("spanish", LayoutLanguage::Dictionary("es_ES")),
    ("latinamerican", LayoutLanguage::Dictionary("es_ES")),
    ("british", LayoutLanguage::Dictionary("en_GB")),
    ("irish", LayoutLanguage::Dictionary("en_GB")),
    ("australian", LayoutLanguage::Dictionary("en_GB")),
    ("us", LayoutLanguage::AnyEnglish),
    ("abc", LayoutLanguage::AnyEnglish),
    ("canadian", LayoutLanguage::AnyEnglish),
    ("dvorak", LayoutLanguage::AnyEnglish),
    ("colemak", LayoutLanguage::AnyEnglish),
];

const BRITISH_SPELLING_REGIONS: [&str; 6] = ["GB", "IE", "AU", "NZ", "ZA", "IN"];

/// The dictionary for the keyboard layout in use, falling back to the
/// system locale, then to US English.
pub fn current_input_dictionary() -> String {
    dictionary_for(current_layout().as_deref(), system_locale()).to_owned()
}

pub fn dictionary_for(layout: Option<&str>, locale: Option<&str>) -> &'static str {
    let from_locale = locale.and_then(dictionary_for_locale);
    match layout.and_then(layout_language) {
        Some(LayoutLanguage::Dictionary(id)) => id,
        Some(LayoutLanguage::AnyEnglish) => from_locale
            .filter(|id| id.starts_with("en_"))
            .unwrap_or(DEFAULT_DICTIONARY),
        None => from_locale.unwrap_or(DEFAULT_DICTIONARY),
    }
}

fn layout_language(layout_id: &str) -> Option<LayoutLanguage> {
    let name: String = layout_id
        .rsplit('.')
        .next()?
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    LAYOUT_PREFIXES
        .into_iter()
        .filter(|(prefix, _)| name.starts_with(prefix))
        .max_by_key(|(prefix, _)| prefix.len())
        .map(|(_, language)| language)
}

/// Accepts `en_IE.UTF-8`, `en-IE`, `de_AT@euro` and bare `fr`.
fn dictionary_for_locale(locale: &str) -> Option<&'static str> {
    let tag = locale.split(['.', '@']).next()?;
    let mut parts = tag.split(['_', '-']);
    let language = parts.next()?.to_ascii_lowercase();
    let region = parts.next().unwrap_or_default().to_ascii_uppercase();
    match language.as_str() {
        "en" if BRITISH_SPELLING_REGIONS.contains(&region.as_str()) => Some("en_GB"),
        "en" => Some("en_US"),
        "de" => Some("de_DE"),
        "fr" => Some("fr_FR"),
        "es" => Some("es_ES"),
        "ru" => Some("ru_RU"),
        _ => None,
    }
}

/// Only macOS exposes the active layout without platform bindings; elsewhere
/// the locale stands in for it.
fn current_layout() -> Option<String> {
    if cfg!(target_os = "macos") {
        read_macos_default(&[
            "com.apple.HIToolbox",
            "AppleCurrentKeyboardLayoutInputSourceID",
        ])
    } else {
        None
    }
}

fn system_locale() -> Option<&'static str> {
    static LOCALE: OnceLock<Option<String>> = OnceLock::new();
    LOCALE
        .get_or_init(|| {
            ["LC_ALL", "LC_MESSAGES", "LANG"]
                .into_iter()
                .filter_map(|key| std::env::var(key).ok())
                .find(|value| !value.is_empty() && value != "C" && value != "POSIX")
                .or_else(|| {
                    cfg!(target_os = "macos")
                        .then(|| read_macos_default(&["-g", "AppleLocale"]))
                        .flatten()
                })
        })
        .as_deref()
}

fn read_macos_default(args: &[&str]) -> Option<String> {
    let output = Command::new("/usr/bin/defaults")
        .arg("read")
        .args(args)
        .output()
        .ok()?;
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_layouts_pick_their_dictionary_whatever_the_locale() {
        let cases = [
            ("com.apple.keylayout.RussianWin", "ru_RU"),
            ("com.apple.keylayout.Russian-Phonetic", "ru_RU"),
            ("com.apple.keylayout.Swiss German", "de_DE"),
            ("com.apple.keylayout.Canadian-CSA", "fr_FR"),
            ("com.apple.keylayout.LatinAmerican", "es_ES"),
            ("com.apple.keylayout.Irish", "en_GB"),
            ("com.apple.keylayout.British-PC", "en_GB"),
        ];
        for (layout, dictionary) in cases {
            assert_eq!(
                dictionary_for(Some(layout), Some("en_US.UTF-8")),
                dictionary,
                "{layout}"
            );
        }
    }

    #[test]
    fn english_layouts_take_the_spelling_variant_from_the_locale() {
        assert_eq!(
            dictionary_for(Some("com.apple.keylayout.ABC"), Some("en_IE.UTF-8")),
            "en_GB"
        );
        assert_eq!(
            dictionary_for(Some("com.apple.keylayout.US"), Some("en-US")),
            "en_US"
        );
        assert_eq!(
            dictionary_for(Some("com.apple.keylayout.US"), Some("de_DE")),
            "en_US"
        );
        assert_eq!(
            dictionary_for(Some("com.apple.keylayout.Canadian"), None),
            "en_US"
        );
    }

    #[test]
    fn unknown_or_missing_layouts_fall_back_to_the_locale() {
        assert_eq!(
            dictionary_for(Some("com.apple.keylayout.Ukrainian"), Some("ru_RU")),
            "ru_RU"
        );
        assert_eq!(dictionary_for(None, Some("de_AT@euro")), "de_DE");
        assert_eq!(dictionary_for(None, Some("fr")), "fr_FR");
        assert_eq!(dictionary_for(None, Some("nl_NL.UTF-8")), "en_US");
        assert_eq!(dictionary_for(None, None), "en_US");
    }
}
