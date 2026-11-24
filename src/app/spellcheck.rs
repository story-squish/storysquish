use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui::{self, Color32, Painter, Pos2, Response, Stroke, Ui, text::CCursor};

use crate::spelling::{self, DictionarySource, KnownWords, Speller};
use crate::thesaurus::{self, Meaning, Thesaurus, ThesaurusSource};

const SQUIGGLE_STEP: f32 = 2.5;
const SQUIGGLE_HEIGHT: f32 = 1.5;
const SYNONYM_MENU_HEIGHT: f32 = 360.0;

type Misspellings = Vec<(Range<usize>, String)>;

#[derive(Default)]
pub(super) enum Status {
    #[default]
    Off,
    Loading(Receiver<Result<Speller, String>>),
    Downloading {
        name: String,
        receiver: Receiver<Result<Speller, String>>,
    },
    Ready(Box<Speller>),
    Failed(String),
}

#[derive(Default)]
pub(super) enum ThesaurusStatus {
    #[default]
    Off,
    Loading {
        source: &'static ThesaurusSource,
        receiver: Receiver<Result<Thesaurus, String>>,
    },
    Ready(Box<Thesaurus>),
    Failed(String),
}

enum Suggestions {
    Corrections(Vec<String>),
    Synonyms(Vec<Meaning>),
}

struct WordMenu {
    field: egui::Id,
    chars: Range<usize>,
    word: String,
    suggestions: Suggestions,
}

#[derive(Default)]
pub(super) struct Spellcheck {
    pub status: Status,
    known: KnownWords,
    by_field: HashMap<egui::Id, (u64, Misspellings)>,
    menu: Option<WordMenu>,
    word_to_add: Option<String>,
    download_base: Option<String>,
    dictionary: Option<String>,
    spare_speller: Option<(String, Box<Speller>)>,
    pub thesaurus: ThesaurusStatus,
    thesaurus_source: Option<&'static ThesaurusSource>,
    spare_thesaurus: Option<(&'static ThesaurusSource, Box<Thesaurus>)>,
    thesaurus_download_base: Option<String>,
}

impl Spellcheck {
    pub fn set_download_base(&mut self, base: &str) {
        self.download_base = Some(base.to_owned());
    }

    pub fn set_thesaurus_download_base(&mut self, base: &str) {
        self.thesaurus_download_base = Some(base.to_owned());
    }

    /// Downloads and parses off the UI thread so switching dictionaries never
    /// stalls typing. A downloaded dictionary is saved into the first of `dirs`.
    /// The previous dictionary is kept aside, so switching back and forth
    /// between two keyboard layouts parses each only once.
    pub fn load(&mut self, ctx: &egui::Context, dictionary: Option<&str>, dirs: &[PathBuf]) {
        if dictionary.is_some()
            && dictionary == self.dictionary.as_deref()
            && !matches!(self.status, Status::Failed(_))
        {
            return;
        }
        self.by_field.clear();
        self.menu = None;
        let previous = std::mem::take(&mut self.status);
        let previous_id = std::mem::replace(&mut self.dictionary, dictionary.map(str::to_owned));
        let spare = match (previous, previous_id) {
            (Status::Ready(speller), Some(id)) => self.spare_speller.replace((id, speller)),
            _ => None,
        }
        .or_else(|| {
            self.spare_speller
                .take_if(|(id, _)| Some(id.as_str()) == dictionary)
        });
        let Some(dictionary) = dictionary else {
            return;
        };
        if let Some((_, speller)) = spare.filter(|(id, _)| id == dictionary) {
            self.status = Status::Ready(speller);
            return;
        }
        let Some(info) = spelling::available_dictionaries(dirs)
            .into_iter()
            .find(|info| info.id == dictionary)
        else {
            self.status = Status::Failed(format!("The {dictionary} dictionary is not installed."));
            return;
        };
        let download_to = match (&info.source, dirs.first()) {
            (DictionarySource::Downloadable, None) => {
                self.status = Status::Failed("No folder to download dictionaries into.".to_owned());
                return;
            }
            (DictionarySource::Downloadable, Some(dir)) => Some(dir.clone()),
            _ => None,
        };
        let download_base = self
            .download_base
            .clone()
            .unwrap_or_else(|| spelling::DOWNLOAD_BASE.to_owned());
        let (sender, receiver) = mpsc::channel();
        self.status = match download_to {
            Some(_) => Status::Downloading {
                name: info.name.clone(),
                receiver,
            },
            None => Status::Loading(receiver),
        };
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let dictionary = match download_to {
                Some(dir) => spelling::download_dictionary(&info.id, &download_base, &dir),
                None => spelling::load_dictionary(&info),
            };
            let loaded = dictionary.map(Speller::new).map_err(|e| format!("{e:#}"));
            if sender.send(loaded).is_ok() {
                ctx.request_repaint();
            }
        });
    }

    /// Returns whether a dictionary finished loading or downloading, so
    /// the list of dictionaries can be refreshed.
    pub fn receive_loaded(&mut self) -> bool {
        let (Status::Loading(receiver) | Status::Downloading { receiver, .. }) = &self.status
        else {
            return false;
        };
        self.status = match receiver.try_recv() {
            Ok(Ok(speller)) => Status::Ready(Box::new(speller)),
            Ok(Err(error)) => Status::Failed(error),
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => {
                Status::Failed("Loading the dictionary stopped unexpectedly.".to_owned())
            }
        };
        self.by_field.clear();
        true
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.status, Status::Ready(_))
    }

    /// Loads, downloading into `dir` the first time, the thesaurus for the
    /// dictionary's language. Keeps the current one when the language is
    /// unchanged, so switching between English variants downloads nothing.
    pub fn load_thesaurus(
        &mut self,
        ctx: &egui::Context,
        dictionary: Option<&str>,
        enabled: bool,
        dir: Option<&Path>,
    ) {
        let source = dictionary
            .filter(|_| enabled)
            .and_then(thesaurus::source_for_dictionary);
        let same_source = source.map(|s| s.name) == self.thesaurus_source.map(|s| s.name);
        if same_source && !matches!(self.thesaurus, ThesaurusStatus::Failed(_)) {
            return;
        }
        let previous = std::mem::take(&mut self.thesaurus);
        let previous_source = std::mem::replace(&mut self.thesaurus_source, source);
        let spare = match (previous, previous_source) {
            (ThesaurusStatus::Ready(thesaurus), Some(previous_source)) => {
                self.spare_thesaurus.replace((previous_source, thesaurus))
            }
            _ => None,
        }
        .or_else(|| {
            self.spare_thesaurus
                .take_if(|(spare, _)| Some(spare.name) == source.map(|s| s.name))
        });
        self.menu = None;
        let Some(source) = source else {
            return;
        };
        if let Some((_, thesaurus)) = spare.filter(|(spare, _)| spare.name == source.name) {
            self.thesaurus = ThesaurusStatus::Ready(thesaurus);
            return;
        }
        let Some(dir) = dir.map(Path::to_owned) else {
            self.thesaurus =
                ThesaurusStatus::Failed("No folder to download the thesaurus into.".to_owned());
            return;
        };
        let base = self
            .thesaurus_download_base
            .clone()
            .unwrap_or_else(|| thesaurus::DOWNLOAD_BASE.to_owned());
        let (sender, receiver) = mpsc::channel();
        self.thesaurus = ThesaurusStatus::Loading { source, receiver };
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let loaded =
                thesaurus::load_or_download(source, &base, &dir).map_err(|e| format!("{e:#}"));
            if sender.send(loaded).is_ok() {
                ctx.request_repaint();
            }
        });
    }

    pub fn receive_thesaurus(&mut self) {
        let ThesaurusStatus::Loading { receiver, .. } = &self.thesaurus else {
            return;
        };
        self.thesaurus = match receiver.try_recv() {
            Ok(Ok(thesaurus)) => ThesaurusStatus::Ready(Box::new(thesaurus)),
            Ok(Err(error)) => ThesaurusStatus::Failed(error),
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                ThesaurusStatus::Failed("Loading the thesaurus stopped unexpectedly.".to_owned())
            }
        };
    }

    pub fn thesaurus_ready(&self) -> bool {
        matches!(self.thesaurus, ThesaurusStatus::Ready(_))
    }

    pub fn set_known_words(&mut self, known: KnownWords) {
        if known != self.known {
            self.known = known;
            self.by_field.clear();
        }
    }

    pub fn take_word_to_add(&mut self) -> Option<String> {
        self.word_to_add.take()
    }

    /// Underlines misspellings in an edited field and, on right-click,
    /// offers corrections for a misspelled word or synonyms for any other.
    /// A replacement marks the returned response as changed.
    pub fn check_field(
        &mut self,
        ui: &Ui,
        output: egui::text_edit::TextEditOutput,
        text: &mut String,
        color: Color32,
    ) -> Response {
        let field = output.response.response.id;
        self.underline_misspellings(ui, &output, field, text, color);

        if output.response.response.secondary_clicked() {
            let clicked_at = ui.input(|input| input.pointer.interact_pos());
            self.menu = clicked_at.and_then(|clicked_at| {
                let under: usize = output
                    .galley
                    .cursor_from_pos(clicked_at - output.galley_pos)
                    .index
                    .into();
                self.word_menu(field, text, under)
            });
        }

        let mut response = output.response.response;
        let Some(menu) = self.menu.as_ref().filter(|menu| menu.field == field) else {
            return response;
        };
        let mut replacement = None;
        let mut add_word = false;
        response.context_menu(|ui| match &menu.suggestions {
            Suggestions::Corrections(suggestions) => {
                replacement = corrections_menu(ui, suggestions);
                ui.separator();
                add_word = ui
                    .button(format!("Add “{}” to dictionary", menu.word))
                    .clicked();
            }
            Suggestions::Synonyms(meanings) => {
                replacement = synonyms_menu(ui, &menu.word, meanings);
            }
        });
        if let Some(replacement) = replacement
            && spelling::replace_word(text, menu.chars.clone(), &menu.word, &replacement)
        {
            response.mark_changed();
        }
        if add_word {
            self.word_to_add = Some(menu.word.clone());
        }
        response
    }

    fn underline_misspellings(
        &mut self,
        ui: &Ui,
        output: &egui::text_edit::TextEditOutput,
        field: egui::Id,
        text: &str,
        color: Color32,
    ) {
        let Status::Ready(speller) = &mut self.status else {
            return;
        };
        let text_hash = hash_of(text);
        let misspellings = match self.by_field.get(&field) {
            Some((hash, misspellings)) if *hash == text_hash => misspellings,
            _ => {
                let found = spelling::misspellings(text, speller, &self.known);
                &self
                    .by_field
                    .entry(field)
                    .insert_entry((text_hash, found))
                    .into_mut()
                    .1
            }
        };

        let painter = ui.painter().with_clip_rect(output.text_clip_rect);
        for (chars, _) in misspellings {
            let start = output.galley.pos_from_cursor(CCursor::new(chars.start));
            let end = output.galley.pos_from_cursor(CCursor::new(chars.end));
            let wraps_onto_next_row = (start.min.y - end.min.y).abs() > 1.0;
            if wraps_onto_next_row {
                for row in &output.galley.rows {
                    let r = row.rect();
                    let y_matches = r.min.y <= start.min.y && start.min.y <= r.max.y
                        || r.min.y <= end.min.y && end.min.y <= r.max.y;
                    if !y_matches {
                        continue;
                    }
                    let is_start_row = (r.min.y - start.min.y).abs() <= 1.0;
                    let is_end_row = (r.min.y - end.min.y).abs() <= 1.0;
                    let x_start = if is_start_row { start.min.x } else { r.min.x };
                    let x_end = if is_end_row { end.min.x } else { r.max.x };
                    let width = x_end - x_start;
                    if width > 0.0 {
                        let baseline = output.galley_pos + egui::vec2(x_start, r.max.y);
                        paint_squiggle(&painter, baseline, width, color);
                    }
                }
            } else {
                let baseline = output.galley_pos + start.left_bottom().to_vec2();
                paint_squiggle(&painter, baseline, end.min.x - start.min.x, color);
            }
        }
    }

    /// `under` is the character index right-clicked; a word ending exactly
    /// there counts, so clicking just past a word still picks it.
    fn word_menu(&self, field: egui::Id, text: &str, under: usize) -> Option<WordMenu> {
        let touches = |chars: &Range<usize>| chars.start <= under && under <= chars.end;
        if let Status::Ready(speller) = &self.status
            && let Some((text_hash, misspellings)) = self.by_field.get(&field)
            && *text_hash == hash_of(text)
            && let Some((chars, word)) = misspellings.iter().find(|(chars, _)| touches(chars))
        {
            return Some(WordMenu {
                field,
                chars: chars.clone(),
                word: word.clone(),
                suggestions: Suggestions::Corrections(speller.suggestions(word)),
            });
        }
        let ThesaurusStatus::Ready(thesaurus) = &self.thesaurus else {
            return None;
        };
        let (chars, word) = spelling::words(text).find(|(chars, _)| touches(chars))?;
        Some(WordMenu {
            field,
            chars,
            word: word.to_owned(),
            suggestions: Suggestions::Synonyms(thesaurus.meanings(word)),
        })
    }
}

fn corrections_menu(ui: &mut Ui, suggestions: &[String]) -> Option<String> {
    if suggestions.is_empty() {
        ui.weak("No suggestions");
    }
    let mut chosen = None;
    for suggestion in suggestions {
        if ui.button(suggestion).clicked() {
            chosen = Some(suggestion.clone());
        }
    }
    chosen
}

fn synonyms_menu(ui: &mut Ui, word: &str, meanings: &[Meaning]) -> Option<String> {
    if meanings.is_empty() {
        ui.weak(format!("No synonyms for “{word}”"));
        return None;
    }
    let mut chosen = None;
    egui::ScrollArea::vertical()
        .max_height(SYNONYM_MENU_HEIGHT)
        .show(ui, |ui| {
            for (index, meaning) in meanings.iter().enumerate() {
                if index > 0 {
                    ui.separator();
                }
                if !meaning.part_of_speech.is_empty() {
                    ui.weak(&meaning.part_of_speech);
                }
                for synonym in &meaning.synonyms {
                    let synonym = thesaurus::match_case(word, synonym);
                    if ui.button(&synonym).clicked() {
                        chosen = Some(synonym);
                    }
                }
            }
        });
    chosen
}

fn hash_of(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn paint_squiggle(painter: &Painter, start: Pos2, width: f32, color: Color32) {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a word is a few hundred points wide at most"
    )]
    let steps = (width / SQUIGGLE_STEP).ceil().max(1.0) as usize;
    let points = (0..=steps)
        .map(|step| {
            #[expect(clippy::cast_precision_loss, reason = "steps is small")]
            let x = (step as f32 * SQUIGGLE_STEP).min(width);
            let y = if step % 2 == 0 { 0.0 } else { SQUIGGLE_HEIGHT };
            start + egui::vec2(x, y)
        })
        .collect();
    painter.line(points, Stroke::new(1.0, color));
}
