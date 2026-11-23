use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui::{self, Color32, Painter, Pos2, Response, Stroke, Ui, text::CCursor};

use crate::spelling::{self, KnownWords, Speller};

const SQUIGGLE_STEP: f32 = 2.5;
const SQUIGGLE_HEIGHT: f32 = 1.5;

type Misspellings = Vec<(Range<usize>, String)>;

#[derive(Default)]
pub(super) enum Status {
    #[default]
    Off,
    Loading(Receiver<Result<Speller, String>>),
    Ready(Box<Speller>),
    Failed(String),
}

struct Correction {
    field: egui::Id,
    chars: Range<usize>,
    word: String,
    suggestions: Vec<String>,
}

#[derive(Default)]
pub(super) struct Spellcheck {
    pub status: Status,
    known: KnownWords,
    by_field: HashMap<egui::Id, (u64, Misspellings)>,
    correction: Option<Correction>,
    word_to_add: Option<String>,
}

impl Spellcheck {
    /// Parses off the UI thread so switching dictionaries never stalls typing.
    pub fn load(&mut self, ctx: &egui::Context, dictionary: Option<&str>, dirs: &[PathBuf]) {
        self.by_field.clear();
        self.correction = None;
        let Some(dictionary) = dictionary else {
            self.status = Status::Off;
            return;
        };
        let Some(info) = spelling::available_dictionaries(dirs)
            .into_iter()
            .find(|info| info.id == dictionary)
        else {
            self.status = Status::Failed(format!("The {dictionary} dictionary is not installed."));
            return;
        };
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let loaded = spelling::load_dictionary(&info)
                .map(Speller::new)
                .map_err(|e| format!("{e:#}"));
            if sender.send(loaded).is_ok() {
                ctx.request_repaint();
            }
        });
        self.status = Status::Loading(receiver);
    }

    pub fn receive_loaded(&mut self) {
        let Status::Loading(receiver) = &self.status else {
            return;
        };
        self.status = match receiver.try_recv() {
            Ok(Ok(speller)) => Status::Ready(Box::new(speller)),
            Ok(Err(error)) => Status::Failed(error),
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Status::Failed("Loading the dictionary stopped unexpectedly.".to_owned())
            }
        };
        self.by_field.clear();
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.status, Status::Ready(_))
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

    /// Underlines misspellings in an edited field and offers corrections on
    /// right-click. A correction marks the returned response as changed.
    pub fn check_field(
        &mut self,
        ui: &Ui,
        output: egui::text_edit::TextEditOutput,
        text: &mut String,
        color: Color32,
    ) -> Response {
        let mut response = output.response.response;
        let Status::Ready(speller) = &mut self.status else {
            return response;
        };
        let field = response.id;
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
            if !wraps_onto_next_row {
                let baseline = output.galley_pos + start.left_bottom().to_vec2();
                paint_squiggle(&painter, baseline, end.min.x - start.min.x, color);
            }
        }

        if response.secondary_clicked() {
            let clicked_at = ui.input(|input| input.pointer.interact_pos());
            self.correction = clicked_at.and_then(|clicked_at| {
                let under: usize = output
                    .galley
                    .cursor_from_pos(clicked_at - output.galley_pos)
                    .index
                    .into();
                let (chars, word) = misspellings
                    .iter()
                    .find(|(chars, _)| chars.start <= under && under <= chars.end)?;
                Some(Correction {
                    field,
                    chars: chars.clone(),
                    word: word.clone(),
                    suggestions: speller.suggestions(word),
                })
            });
        }

        let Some(correction) = self.correction.as_ref().filter(|c| c.field == field) else {
            return response;
        };
        let mut replacement = None;
        let mut add_word = false;
        response.context_menu(|ui| {
            if correction.suggestions.is_empty() {
                ui.weak("No suggestions");
            }
            for suggestion in &correction.suggestions {
                if ui.button(suggestion).clicked() {
                    replacement = Some(suggestion.clone());
                }
            }
            ui.separator();
            add_word = ui
                .button(format!("Add “{}” to dictionary", correction.word))
                .clicked();
        });
        if let Some(replacement) = replacement
            && spelling::replace_word(
                text,
                correction.chars.clone(),
                &correction.word,
                &replacement,
            )
        {
            response.mark_changed();
        }
        if add_word {
            self.word_to_add = Some(correction.word.clone());
        }
        response
    }
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
