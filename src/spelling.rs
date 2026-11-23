use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use spellbook::Dictionary;

pub const DEFAULT_DICTIONARY: &str = "en_US";
const SUGGESTION_LIMIT: usize = 6;

struct BundledDictionary {
    id: &'static str,
    name: &'static str,
    aff: &'static str,
    dic: &'static str,
}

const BUNDLED: [BundledDictionary; 2] = [
    BundledDictionary {
        id: "en_US",
        name: "English (US)",
        aff: include_str!("../assets/dictionaries/en_US/en_US.aff"),
        dic: include_str!("../assets/dictionaries/en_US/en_US.dic"),
    },
    BundledDictionary {
        id: "en_GB",
        name: "English (UK)",
        aff: include_str!("../assets/dictionaries/en_GB/en_GB.aff"),
        dic: include_str!("../assets/dictionaries/en_GB/en_GB.dic"),
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictionarySource {
    Bundled,
    Installed { aff: PathBuf, dic: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryInfo {
    pub id: String,
    pub name: String,
    pub source: DictionarySource,
}

/// Bundled dictionaries first, then Hunspell `.aff`/`.dic` pairs found in
/// `dirs`, earlier directories winning when two share an id.
pub fn available_dictionaries(dirs: &[PathBuf]) -> Vec<DictionaryInfo> {
    let mut found: Vec<DictionaryInfo> = BUNDLED
        .iter()
        .map(|bundled| DictionaryInfo {
            id: bundled.id.to_owned(),
            name: bundled.name.to_owned(),
            source: DictionarySource::Bundled,
        })
        .collect();
    let mut installed = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        let mut in_dir: Vec<DictionaryInfo> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter_map(|dic| installed_dictionary(&dic))
            .collect();
        in_dir.sort_by(|a, b| a.id.cmp(&b.id));
        installed.extend(in_dir);
    }
    for dictionary in installed {
        if !found.iter().any(|known| known.id == dictionary.id) {
            found.push(dictionary);
        }
    }
    found
}

fn installed_dictionary(dic: &Path) -> Option<DictionaryInfo> {
    if dic.extension()? != "dic" {
        return None;
    }
    let aff = dic.with_extension("aff");
    if !aff.is_file() {
        return None;
    }
    let id = dic.file_stem()?.to_str()?.to_owned();
    Some(DictionaryInfo {
        name: format!("{id} (installed)"),
        id,
        source: DictionarySource::Installed {
            aff,
            dic: dic.to_owned(),
        },
    })
}

/// Where Hunspell dictionaries are commonly installed, after the app's own
/// `dictionaries` folder so a user's copy wins over the system's.
pub fn dictionary_dirs(config_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![config_dir.join("dictionaries")];
    if let Some(home) = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_owned()) {
        dirs.push(home.join("Library/Spelling"));
        dirs.push(home.join(".local/share/hunspell"));
    }
    dirs.extend(
        [
            "/Library/Spelling",
            "/opt/homebrew/share/hunspell",
            "/usr/local/share/hunspell",
            "/usr/share/hunspell",
            "/usr/share/myspell",
            "/usr/share/myspell/dicts",
        ]
        .map(PathBuf::from),
    );
    dirs
}

pub fn load_dictionary(info: &DictionaryInfo) -> Result<Dictionary> {
    let parsed = match &info.source {
        DictionarySource::Bundled => {
            let bundled = BUNDLED
                .iter()
                .find(|bundled| bundled.id == info.id)
                .with_context(|| format!("no bundled dictionary {}", info.id))?;
            Dictionary::new(bundled.aff, bundled.dic)
        }
        DictionarySource::Installed { aff, dic } => {
            Dictionary::new(&read_dictionary_file(aff)?, &read_dictionary_file(dic)?)
        }
    };
    parsed.map_err(|e| anyhow!("{} is not a valid Hunspell dictionary: {e}", info.id))
}

/// Older Hunspell dictionaries are ISO-8859-1 rather than UTF-8.
fn read_dictionary_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(String::from_utf8(bytes)
        .unwrap_or_else(|e| e.into_bytes().into_iter().map(char::from).collect()))
}

pub struct Speller {
    dictionary: Dictionary,
    verdicts: HashMap<String, bool>,
}

impl Speller {
    pub fn new(dictionary: Dictionary) -> Self {
        Self {
            dictionary,
            verdicts: HashMap::new(),
        }
    }

    pub fn is_correct(&mut self, word: &str) -> bool {
        if let Some(&correct) = self.verdicts.get(word) {
            return correct;
        }
        let correct = self.dictionary.check(&normalize_apostrophes(word));
        self.verdicts.insert(word.to_owned(), correct);
        correct
    }

    pub fn suggestions(&self, word: &str) -> Vec<String> {
        let mut suggestions = Vec::new();
        self.dictionary
            .suggest(&normalize_apostrophes(word), &mut suggestions);
        suggestions.truncate(SUGGESTION_LIMIT);
        suggestions
    }
}

fn normalize_apostrophes(word: &str) -> String {
    word.replace('’', "'")
}

/// Words the dictionary would not know but the writer means: character and
/// location names and words added through "Add to dictionary".
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct KnownWords(HashSet<String>);

impl KnownWords {
    pub fn new<'a>(personal: &BTreeSet<String>, names: impl IntoIterator<Item = &'a str>) -> Self {
        let personal = personal.iter().map(|word| known_word_key(word));
        let names = names
            .into_iter()
            .flat_map(|name| words(name).map(|(_, word)| known_word_key(word)));
        Self(personal.chain(names).collect())
    }

    pub fn contains(&self, word: &str) -> bool {
        self.0.contains(&known_word_key(word))
    }
}

fn known_word_key(word: &str) -> String {
    let word = normalize_apostrophes(word).to_lowercase();
    match word.strip_suffix("'s") {
        Some(stem) if !stem.is_empty() => stem.to_owned(),
        _ => word,
    }
}

/// Character ranges of words in `text`, not byte ranges, so they line up
/// with text-editor cursors.
pub fn words(text: &str) -> impl Iterator<Item = (Range<usize>, &str)> {
    let mut chars = text.char_indices().enumerate().peekable();
    std::iter::from_fn(move || {
        loop {
            let (start_char, (start_byte, first)) = chars.next()?;
            if !first.is_alphanumeric() {
                continue;
            }
            let (mut end_char, mut end_byte) = (start_char + 1, start_byte + first.len_utf8());
            while let Some(&(index, (byte, c))) = chars.peek() {
                let continues = c.is_alphanumeric()
                    || (is_apostrophe(c)
                        && text[byte + c.len_utf8()..]
                            .chars()
                            .next()
                            .is_some_and(char::is_alphabetic));
                if !continues {
                    break;
                }
                chars.next();
                end_char = index + 1;
                end_byte = byte + c.len_utf8();
            }
            return Some((start_char..end_char, &text[start_byte..end_byte]));
        }
    })
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '’')
}

/// Numbers, codes and acronyms are not prose and stay unchecked.
fn worth_checking(word: &str) -> bool {
    let has_digit = word.chars().any(|c| c.is_ascii_digit());
    let letters = word.chars().filter(|c| c.is_alphabetic()).count();
    let is_acronym = letters > 1 && word.chars().all(|c| !c.is_lowercase());
    !has_digit && !is_acronym
}

pub fn misspellings(
    text: &str,
    speller: &mut Speller,
    known: &KnownWords,
) -> Vec<(Range<usize>, String)> {
    words(text)
        .filter(|(_, word)| worth_checking(word))
        .filter(|(_, word)| !known.contains(word) && !speller.is_correct(word))
        .map(|(range, word)| (range, word.to_owned()))
        .collect()
}

/// Returns `false`, leaving `text` alone, when `chars` no longer holds
/// `expected`, e.g. because the text changed while a menu was open.
pub fn replace_word(text: &mut String, chars: Range<usize>, expected: &str, with: &str) -> bool {
    let byte_at = |char_index: usize| {
        text.char_indices()
            .nth(char_index)
            .map_or(text.len(), |(byte, _)| byte)
    };
    let bytes = byte_at(chars.start)..byte_at(chars.end);
    if text.get(bytes.clone()) != Some(expected) {
        return false;
    }
    text.replace_range(bytes, with);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundled(id: &str) -> Speller {
        let info = available_dictionaries(&[])
            .into_iter()
            .find(|info| info.id == id)
            .unwrap();
        Speller::new(load_dictionary(&info).unwrap())
    }

    fn word_list(text: &str) -> Vec<&str> {
        words(text).map(|(_, word)| word).collect()
    }

    #[test]
    fn words_keep_inner_apostrophes_and_split_on_hyphens() {
        assert_eq!(
            word_list("Don’t go—it's O'Brien's well-known 'secret'."),
            [
                "Don’t",
                "go",
                "it's",
                "O'Brien's",
                "well",
                "known",
                "secret"
            ]
        );
    }

    #[test]
    fn word_ranges_count_characters_not_bytes() {
        let ranges: Vec<_> = words("é café").map(|(range, _)| range).collect();
        assert_eq!(ranges, [0..1, 2..6]);
    }

    #[test]
    fn misspellings_skip_numbers_acronyms_and_known_names() {
        let mut speller = bundled("en_US");
        let known = KnownWords::new(
            &BTreeSet::from(["grimdark".to_owned()]),
            ["Aelin Galathynius"],
        );
        let found = misspellings(
            "The NASA probe in 1984 saw Aelin's grimdark wrold, Galathynius.",
            &mut speller,
            &known,
        );
        assert_eq!(found, [(44..49, "wrold".to_owned())]);
    }

    #[test]
    fn regional_dictionaries_differ() {
        assert!(bundled("en_US").is_correct("color"));
        assert!(!bundled("en_US").is_correct("colour"));
        assert!(bundled("en_GB").is_correct("colour"));
    }

    #[test]
    fn suggestions_include_the_likely_word() {
        let speller = bundled("en_US");
        assert!(speller.suggestions("wrold").contains(&"world".to_owned()));
    }

    #[test]
    fn replace_word_uses_character_ranges_and_checks_the_word() {
        let mut text = "café wrold".to_owned();
        assert!(!replace_word(&mut text, 5..10, "other", "world"));
        assert!(replace_word(&mut text, 5..10, "wrold", "world"));
        assert_eq!(text, "café world");
    }

    #[test]
    fn installed_dictionaries_need_both_files_and_never_shadow_bundled() {
        let dir = tempfile::tempdir().unwrap();
        for file in [
            "de_DE.aff",
            "de_DE.dic",
            "en_US.aff",
            "en_US.dic",
            "fr_FR.dic",
        ] {
            fs::write(dir.path().join(file), "").unwrap();
        }
        let ids: Vec<_> = available_dictionaries(&[dir.path().to_owned()])
            .into_iter()
            .map(|info| (info.id, info.source == DictionarySource::Bundled))
            .collect();
        assert_eq!(
            ids,
            [
                ("en_US".to_owned(), true),
                ("en_GB".to_owned(), true),
                ("de_DE".to_owned(), false)
            ]
        );
    }

    #[test]
    fn latin1_dictionary_files_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.dic");
        fs::write(&path, [b'c', b'a', b'f', 0xE9]).unwrap();
        assert_eq!(read_dictionary_file(&path).unwrap(), "café");
    }
}
