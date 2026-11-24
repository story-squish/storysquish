use std::collections::{HashMap, HashSet};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::storage::write_atomically;

/// Pinned to a commit so a download is always the file that was tested.
pub const DOWNLOAD_BASE: &str = "https://raw.githubusercontent.com/LibreOffice/dictionaries/\
     c0865a85eb67dc377f2cf8e8fe2c4fa9e53f0dcb";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024;
const SAVED_ENCODING: &str = "UTF-8";

pub struct ThesaurusSource {
    language: &'static str,
    pub name: &'static str,
    pub download_megabytes: u32,
    path: &'static str,
}

const SOURCES: [ThesaurusSource; 5] = [
    ThesaurusSource {
        language: "en",
        name: "English",
        download_megabytes: 18,
        path: "en/th_en_US_v2.dat",
    },
    ThesaurusSource {
        language: "de",
        name: "German",
        download_megabytes: 32,
        path: "de/th_de_DE_v2.dat",
    },
    ThesaurusSource {
        language: "es",
        name: "Spanish",
        download_megabytes: 3,
        path: "es/th_es_v2.dat",
    },
    ThesaurusSource {
        language: "fr",
        name: "French",
        download_megabytes: 5,
        path: "fr_FR/dictionaries/thes_fr.dat",
    },
    ThesaurusSource {
        language: "ru",
        name: "Russian",
        download_megabytes: 3,
        path: "ru_RU/th_ru_RU_M_aot_and_v2.dat",
    },
];

/// The thesaurus matching a dictionary's language, so `en_GB` and an
/// installed `en_AU` both get the English one.
pub fn source_for_dictionary(dictionary_id: &str) -> Option<&'static ThesaurusSource> {
    let language = dictionary_id
        .split(['_', '-'])
        .next()
        .unwrap_or(dictionary_id);
    SOURCES
        .iter()
        .find(|source| source.language.eq_ignore_ascii_case(language))
}

fn saved_path(source: &ThesaurusSource, dir: &Path) -> PathBuf {
    dir.join(format!("th_{}.dat", source.language))
}

/// Reads the thesaurus saved in `dir`, downloading it from `base` first
/// when it is not there yet.
pub fn load_or_download(source: &ThesaurusSource, base: &str, dir: &Path) -> Result<Thesaurus> {
    let path = saved_path(source, dir);
    if path.is_file() {
        let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        return Thesaurus::parse(&bytes)
            .with_context(|| format!("{} is not a valid thesaurus", path.display()));
    }
    let thesaurus = download(source, base)?;
    write_atomically(&path, &thesaurus.text)?;
    Ok(thesaurus)
}

fn download(source: &ThesaurusSource, base: &str) -> Result<Thesaurus> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(DOWNLOAD_TIMEOUT))
        .build()
        .into();
    let url = format!("{}/{}", base.trim_end_matches('/'), source.path);
    let bytes = agent
        .get(&url)
        .header("User-Agent", crate::APP_NAME)
        .call()
        .and_then(|mut response| {
            response
                .body_mut()
                .with_config()
                .limit(MAX_DOWNLOAD_BYTES)
                .read_to_vec()
        })
        .with_context(|| format!("downloading the {} thesaurus", source.name))?;
    Thesaurus::parse(&bytes)
        .with_context(|| format!("the downloaded {} thesaurus is not valid", source.name))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meaning {
    pub part_of_speech: String,
    pub synonyms: Vec<String>,
}

/// A `MyThes` `.dat` file kept as text, with an index from each headword to
/// its meaning lines so a lookup parses only the lines it needs.
pub struct Thesaurus {
    text: String,
    entries: HashMap<String, Range<usize>>,
}

impl Thesaurus {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        let header_end = bytes
            .iter()
            .position(|&b| b == b'\n')
            .context("missing the encoding line")?;
        let encoding = String::from_utf8_lossy(&bytes[..header_end]);
        let body = &bytes[header_end + 1..];
        let body = match encoding.trim().to_ascii_uppercase().as_str() {
            "UTF-8" | "UTF8" => String::from_utf8(body.to_vec()).context("invalid UTF-8")?,
            "ISO8859-1" | "ISO-8859-1" | "LATIN1" => body.iter().copied().map(char::from).collect(),
            other => bail!("unsupported encoding {other}"),
        };
        let text = format!("{SAVED_ENCODING}\n{body}");
        let entries = index_entries(&text, SAVED_ENCODING.len() + 1)?;
        Ok(Self { text, entries })
    }

    /// Meanings of `word` with annotations such as "(similar term)"
    /// stripped, antonyms dropped, and each synonym listed only once.
    pub fn meanings(&self, word: &str) -> Vec<Meaning> {
        let Some(range) = self.entries.get(&lookup_key(word)) else {
            return Vec::new();
        };
        let mut seen = HashSet::from([lookup_key(word)]);
        self.text[range.clone()]
            .lines()
            .filter_map(|line| {
                let mut fields = line.trim_end_matches('\r').split('|');
                let part_of_speech = fields.next().map_or_else(String::new, part_of_speech);
                let synonyms: Vec<String> = fields
                    .filter(|synonym| !synonym.trim_end().ends_with("(antonym)"))
                    .map(without_annotations)
                    .filter(|synonym| !synonym.is_empty() && seen.insert(lookup_key(synonym)))
                    .map(str::to_owned)
                    .collect();
                (!synonyms.is_empty()).then_some(Meaning {
                    part_of_speech,
                    synonyms,
                })
            })
            .collect()
    }
}

/// Maps each headword to the byte range of the meaning lines after it. The
/// first entry wins when a headword is listed twice.
fn index_entries(text: &str, body_start: usize) -> Result<HashMap<String, Range<usize>>> {
    let mut entries = HashMap::new();
    let mut lines = text[body_start..].split_inclusive('\n');
    let mut offset = body_start;
    while let Some(header) = lines.next() {
        offset += header.len();
        let header = header.trim_end_matches(['\n', '\r']);
        if header.is_empty() {
            continue;
        }
        let (word, count) = header
            .rsplit_once('|')
            .with_context(|| format!("expected “word|count”, found “{header}”"))?;
        let count: usize = count
            .trim()
            .parse()
            .with_context(|| format!("invalid meaning count in “{header}”"))?;
        let start = offset;
        for _ in 0..count {
            offset += lines
                .next()
                .with_context(|| format!("“{word}” is missing meanings"))?
                .len();
        }
        entries.entry(lookup_key(word)).or_insert(start..offset);
    }
    Ok(entries)
}

fn lookup_key(word: &str) -> String {
    word.trim().replace('’', "'").to_lowercase()
}

fn part_of_speech(field: &str) -> String {
    let field = field.trim();
    let field = field
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
        .unwrap_or(field);
    if field == "-" {
        String::new()
    } else {
        field.to_owned()
    }
}

fn without_annotations(synonym: &str) -> &str {
    let mut synonym = synonym.trim();
    while let Some(open) = synonym
        .strip_suffix(')')
        .and_then(|inner| inner.rfind('('))
        .filter(|&open| open > 0)
    {
        synonym = synonym[..open].trim_end();
    }
    synonym
}

/// Gives `synonym` the capitalisation of the word it replaces, so a
/// sentence-initial or shouted word stays that way.
pub fn match_case(original: &str, synonym: &str) -> String {
    let letters: Vec<char> = original.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() > 1 && letters.iter().all(|c| c.is_uppercase()) {
        return synonym.to_uppercase();
    }
    let starts_upper = original.chars().next().is_some_and(char::is_uppercase);
    let mut chars = synonym.chars();
    match chars.next() {
        Some(first) if starts_upper && first.is_lowercase() => {
            first.to_uppercase().chain(chars).collect()
        }
        _ => synonym.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "UTF-8\n\
        happy|2\n\
        (adj)|blessed (similar term)|glad (related term)|unhappy (antonym)\n\
        (adj)|felicitous|glad|fortunate (similar term)\n\
        rain|1\n\
        -|Regen (ugs.)|rain\n";

    fn sample() -> Thesaurus {
        Thesaurus::parse(SAMPLE.as_bytes()).unwrap()
    }

    #[test]
    fn meanings_strip_annotations_and_drop_antonyms_duplicates_and_the_word() {
        assert_eq!(
            sample().meanings("Happy"),
            [
                Meaning {
                    part_of_speech: "adj".to_owned(),
                    synonyms: vec!["blessed".to_owned(), "glad".to_owned()],
                },
                Meaning {
                    part_of_speech: "adj".to_owned(),
                    synonyms: vec!["felicitous".to_owned(), "fortunate".to_owned()],
                },
            ]
        );
        assert_eq!(
            sample().meanings("rain"),
            [Meaning {
                part_of_speech: String::new(),
                synonyms: vec!["Regen".to_owned()],
            }]
        );
        assert_eq!(sample().meanings("sad"), []);
    }

    #[test]
    fn latin1_bom_and_crlf_files_parse() {
        let latin1 = b"ISO8859-1\r\ncaf\xe9|1\r\n-|bar\r\n";
        assert_eq!(
            Thesaurus::parse(latin1).unwrap().meanings("café")[0].synonyms,
            ["bar"]
        );
        let with_bom = "\u{feff}UTF-8\nмир|1\n(синоним)|вселенная\n";
        assert_eq!(
            Thesaurus::parse(with_bom.as_bytes())
                .unwrap()
                .meanings("Мир")[0]
                .part_of_speech,
            "синоним"
        );
    }

    #[test]
    fn malformed_files_are_rejected() {
        assert!(Thesaurus::parse(b"KOI8-R\nword|1\n-|x\n").is_err());
        assert!(Thesaurus::parse(b"UTF-8\nword|2\n-|x\n").is_err());
        assert!(Thesaurus::parse(b"UTF-8\nno count here\n").is_err());
    }

    #[test]
    fn synonyms_take_the_case_of_the_replaced_word() {
        assert_eq!(match_case("Happy", "glad"), "Glad");
        assert_eq!(match_case("HAPPY", "glad"), "GLAD");
        assert_eq!(match_case("happy", "Glad"), "Glad");
        assert_eq!(match_case("I", "me"), "Me");
    }

    #[test]
    fn dictionaries_map_to_their_language_thesaurus() {
        assert_eq!(source_for_dictionary("en_GB").unwrap().name, "English");
        assert_eq!(source_for_dictionary("de-AT").unwrap().name, "German");
        assert!(source_for_dictionary("nl_NL").is_none());
    }

    fn serve(path: &'static str, body: &'static [u8]) -> String {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut request_line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut request_line)
                    .unwrap();
                let requested = request_line.split_whitespace().nth(1).unwrap_or_default();
                let (status, body) = if requested == path {
                    (200, body)
                } else {
                    (404, &b""[..])
                };
                write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(body).unwrap();
            }
        });
        base
    }

    #[test]
    fn downloads_are_saved_as_utf8_and_reused() {
        let base = serve("/es/th_es_v2.dat", b"ISO8859-1\nalegr\xeda|1\n-|gozo\n");
        let dir = tempfile::tempdir().unwrap();
        let spanish = source_for_dictionary("es_ES").unwrap();

        let downloaded = load_or_download(spanish, &base, dir.path()).unwrap();

        assert_eq!(downloaded.meanings("alegría")[0].synonyms, ["gozo"]);
        let saved = fs::read_to_string(dir.path().join("th_es.dat")).unwrap();
        assert_eq!(saved, "UTF-8\nalegría|1\n-|gozo\n");
        let offline = load_or_download(spanish, "http://127.0.0.1:9", dir.path()).unwrap();
        assert_eq!(offline.meanings("alegría")[0].synonyms, ["gozo"]);
    }

    #[test]
    fn failed_downloads_leave_nothing_behind() {
        let base = serve("/en/th_en_US_v2.dat", b"UTF-8\nbroken|3\n-|x\n");
        let dir = tempfile::tempdir().unwrap();

        let error =
            load_or_download(source_for_dictionary("en_US").unwrap(), &base, dir.path()).err();

        let error = format!("{:#}", error.unwrap());
        assert!(error.contains("English"), "{error}");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
