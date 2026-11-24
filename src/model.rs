use std::collections::{HashMap, HashSet};
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::calendar::{self, StoryDate};

pub const DEFAULT_TITLE: &str = "Untitled Novel";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Status {
    #[default]
    Outline,
    Draft,
    Revision,
    Final,
    Archived,
}

impl Status {
    pub const ALL: [Self; 5] = [
        Self::Outline,
        Self::Draft,
        Self::Revision,
        Self::Final,
        Self::Archived,
    ];
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Act {
    #[serde(rename = "Act I")]
    One,
    #[serde(rename = "Act II")]
    Two,
    #[serde(rename = "Act III")]
    Three,
}

impl Act {
    pub const ALL: [Self; 3] = [Self::One, Self::Two, Self::Three];
}

impl fmt::Display for Act {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::One => "Act I",
            Self::Two => "Act II",
            Self::Three => "Act III",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(String);

impl Id {
    fn generate() -> Self {
        Self(uuid::Uuid::new_v4().simple().to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Character {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub gender: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub notes: String,
}

/// Directed, so "Ada → Bo: mentor" and "Bo → Ada: rival" can differ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relationship {
    pub id: Id,
    pub from: Id,
    pub to: Id,
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scene {
    pub id: Id,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub status: Status,
    #[serde(default, with = "empty_string_as_none")]
    pub pov: Option<Id>,
    #[serde(default, with = "empty_string_as_none")]
    pub act: Option<Act>,
    #[serde(default, with = "empty_string_as_none")]
    pub location: Option<Id>,
    /// Hand-numbered story day, used while the board has no calendar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day: Option<i32>,
    /// Calendar date, used while the board has a calendar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<StoryDate>,
    /// Takes the scene off the timeline without forgetting its day and date.
    #[serde(rename = "dayHidden", default, skip_serializing_if = "is_false")]
    pub day_hidden: bool,
    #[serde(default)]
    pub text: String,
    #[serde(rename = "chapterBreak", default, skip_serializing_if = "is_false")]
    pub chapter_break: bool,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde passes fields by reference"
)]
fn is_false(value: &bool) -> bool {
    !value
}

/// A scene's chapter and whether the chapter opens with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChapterPlace {
    pub number: usize,
    pub starts: bool,
}

impl Scene {
    fn new() -> Self {
        Self {
            id: Id::generate(),
            title: String::new(),
            summary: String::new(),
            status: Status::default(),
            pov: None,
            act: None,
            location: None,
            day: None,
            date: None,
            day_hidden: false,
            text: String::new(),
            chapter_break: false,
        }
    }

    pub fn word_count(&self) -> usize {
        self.text.split_whitespace().count()
    }

    pub fn shown_day(&self) -> Option<i32> {
        self.day.filter(|_| !self.day_hidden)
    }

    pub fn shown_date(&self) -> Option<StoryDate> {
        self.date.filter(|_| !self.day_hidden)
    }
}

/// Where a scene falls in the story's chronology. Archived scenes on a
/// calendar keep their date but get no day number, since they are not
/// part of the story that is counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StoryDay {
    pub number: Option<i32>,
    pub date: Option<StoryDate>,
}

/// Distinct manuscript dates in order, and how they map to day numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarDays {
    dates: Vec<StoryDate>,
    absolute: bool,
}

impl CalendarDays {
    pub fn first(&self) -> Option<StoryDate> {
        self.dates.first().copied()
    }

    pub fn number_of(&self, date: StoryDate) -> Option<i32> {
        let ordinal = if self.absolute {
            self.first()?.days_until(date)
        } else {
            i64::try_from(self.dates.binary_search(&date).ok()?).ok()?
        };
        i32::try_from(ordinal + 1).ok()
    }

    /// The date that would be day `number`. Counting only dates with
    /// scenes, a number past the last day lands that many days after it,
    /// and one before day 1 that many days before it.
    pub fn date_of(&self, number: i32) -> Option<StoryDate> {
        let first = self.first()?;
        let ordinal = i64::from(number) - 1;
        if self.absolute || ordinal < 0 {
            return first.plus_days(ordinal);
        }
        let index = usize::try_from(ordinal).ok()?;
        if let Some(date) = self.dates.get(index) {
            return Some(*date);
        }
        let last_index = i64::try_from(self.dates.len()).ok()? - 1;
        self.dates.last()?.plus_days(ordinal - last_index)
    }
}

impl StoryDay {
    /// `Day 3`, `Day 3 · Wed 18 Mar 2026` or, for archived scenes on a
    /// calendar, `Wed 18 Mar 2026`.
    pub fn label(self) -> String {
        match (self.number, self.date) {
            (Some(number), Some(date)) => format!("Day {number} · {}", date.long_label()),
            (Some(number), None) => format!("Day {number}"),
            (None, Some(date)) => date.long_label(),
            (None, None) => String::new(),
        }
    }
}

/// Indices of manuscript (non-archived) scenes missing each detail.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SceneGaps {
    pub pov: Vec<usize>,
    pub location: Vec<usize>,
    pub day: Vec<usize>,
}

/// On-disk format matches the JSON exported by the original HTML planner,
/// so existing exports import unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "StoredBoard")]
pub struct Board {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub characters: Vec<Character>,
    pub scenes: Vec<Scene>,
    #[serde(default)]
    pub locations: Vec<Location>,
    #[serde(default)]
    pub relationships: Vec<Relationship>,
    /// Target manuscript word count. Older apps drop it on save.
    #[serde(rename = "wordGoal", default, skip_serializing_if = "Option::is_none")]
    pub word_goal: Option<usize>,
    /// Scenes are placed by date, and day 1 is the earliest manuscript date.
    #[serde(default, skip_serializing_if = "is_false")]
    pub calendar: bool,
    /// On a calendar, number days by distance from day 1 rather than
    /// counting only the dates that have scenes.
    #[serde(rename = "absoluteDays", default, skip_serializing_if = "is_false")]
    pub absolute_days: bool,
}

/// Board as read from disk, before migrating older calendar data.
#[derive(Deserialize)]
struct StoredBoard {
    #[serde(default)]
    title: String,
    #[serde(default)]
    characters: Vec<Character>,
    scenes: Vec<Scene>,
    #[serde(default)]
    locations: Vec<Location>,
    #[serde(default)]
    relationships: Vec<Relationship>,
    #[serde(rename = "wordGoal", default)]
    word_goal: Option<usize>,
    #[serde(default)]
    calendar: bool,
    #[serde(rename = "absoluteDays", default)]
    absolute_days: bool,
    /// The first calendar stored a start date and kept scene days as
    /// offsets from it; those become per-scene dates.
    #[serde(rename = "storyStart", default)]
    story_start: Option<StoryDate>,
}

impl From<StoredBoard> for Board {
    fn from(stored: StoredBoard) -> Self {
        let mut scenes = stored.scenes;
        if let Some(start) = stored.story_start {
            for scene in &mut scenes {
                if scene.date.is_none() {
                    scene.date = scene.day.and_then(|day| calendar::date_of_day(start, day));
                }
            }
        }
        Self {
            title: stored.title,
            characters: stored.characters,
            scenes,
            locations: stored.locations,
            relationships: stored.relationships,
            word_goal: stored.word_goal,
            calendar: stored.calendar || stored.story_start.is_some(),
            absolute_days: stored.absolute_days,
        }
    }
}

impl Default for Board {
    fn default() -> Self {
        Self {
            title: DEFAULT_TITLE.to_owned(),
            characters: Vec::new(),
            scenes: Vec::new(),
            locations: Vec::new(),
            relationships: Vec::new(),
            word_goal: None,
            calendar: false,
            absolute_days: false,
        }
    }
}

impl Board {
    /// The title and word goal alone do not count: a new board keeps a
    /// default title, and a goal is no work to lose.
    pub fn is_empty(&self) -> bool {
        self.scenes.is_empty()
            && self.characters.is_empty()
            && self.locations.is_empty()
            && self.relationships.is_empty()
    }

    pub fn has_placeholder_title(&self) -> bool {
        let title = self.title.trim();
        title.is_empty() || title == DEFAULT_TITLE
    }

    pub fn add_scene(&mut self) -> Id {
        let scene = Scene::new();
        let id = scene.id.clone();
        self.scenes.push(scene);
        id
    }

    pub fn scene_index(&self, id: &Id) -> Option<usize> {
        self.scenes.iter().position(|s| &s.id == id)
    }

    pub fn scene_mut(&mut self, id: &Id) -> Option<&mut Scene> {
        self.scenes.iter_mut().find(|s| &s.id == id)
    }

    pub fn delete_scene(&mut self, id: &Id) {
        self.scenes.retain(|s| &s.id != id);
    }

    /// Inserts a copy with a fresh id right after the original.
    pub fn duplicate_scene(&mut self, id: &Id) -> Option<Id> {
        let index = self.scene_index(id)?;
        let original = &self.scenes[index];
        let copy = Scene {
            id: Id::generate(),
            title: if original.title.is_empty() {
                String::new()
            } else {
                format!("{} (copy)", original.title)
            },
            chapter_break: false,
            ..original.clone()
        };
        let copy_id = copy.id.clone();
        self.scenes.insert(index + 1, copy);
        Some(copy_id)
    }

    /// Moves a scene one slot earlier (`-1`) or later (`1`); returns `false`
    /// at either end of the board.
    pub fn shift_scene(&mut self, id: &Id, offset: isize) -> bool {
        shift_by(&mut self.scenes, |s| &s.id == id, offset)
    }

    /// Moves a character one slot up (`-1`) or down (`1`); returns `false`
    /// at either end of the list.
    pub fn shift_character(&mut self, id: &Id, offset: isize) -> bool {
        shift_by(&mut self.characters, |c| &c.id == id, offset)
    }

    pub fn character_index(&self, id: &Id) -> Option<usize> {
        self.characters.iter().position(|c| &c.id == id)
    }

    pub fn move_scene(&mut self, from: usize, to: usize) {
        if from >= self.scenes.len() || to >= self.scenes.len() || from == to {
            return;
        }
        let scene = self.scenes.remove(from);
        self.scenes.insert(to, scene);
    }

    /// Returns `false` when the trimmed name is empty.
    pub fn add_character(&mut self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        self.characters.push(Character {
            id: Id::generate(),
            name: name.to_owned(),
            role: String::new(),
            gender: String::new(),
            goal: String::new(),
            notes: String::new(),
        });
        true
    }

    pub fn delete_character(&mut self, id: &Id) {
        self.characters.retain(|c| &c.id != id);
        self.relationships.retain(|r| &r.from != id && &r.to != id);
        for scene in &mut self.scenes {
            if scene.pov.as_ref() == Some(id) {
                scene.pov = None;
            }
        }
    }

    /// Returns `false` when the trimmed name is empty.
    pub fn add_location(&mut self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        self.locations.push(Location {
            id: Id::generate(),
            name: name.to_owned(),
            notes: String::new(),
        });
        true
    }

    pub fn delete_location(&mut self, id: &Id) {
        self.locations.retain(|l| &l.id != id);
        for scene in &mut self.scenes {
            if scene.location.as_ref() == Some(id) {
                scene.location = None;
            }
        }
    }

    pub fn location(&self, id: &Id) -> Option<&Location> {
        self.locations.iter().find(|l| &l.id == id)
    }

    pub fn location_mut(&mut self, id: &Id) -> Option<&mut Location> {
        self.locations.iter_mut().find(|l| &l.id == id)
    }

    pub fn location_index(&self, id: &Id) -> Option<usize> {
        self.locations.iter().position(|l| &l.id == id)
    }

    /// Moves a location one slot up (`-1`) or down (`1`); returns `false`
    /// at either end of the list.
    pub fn shift_location(&mut self, id: &Id, offset: isize) -> bool {
        shift_by(&mut self.locations, |l| &l.id == id, offset)
    }

    pub fn location_scene_count(&self, id: &Id) -> usize {
        self.scenes
            .iter()
            .filter(|s| s.location.as_ref() == Some(id))
            .count()
    }

    /// Returns `false` for self-links and unknown characters.
    pub fn add_relationship(&mut self, from: &Id, to: &Id, label: &str) -> bool {
        if from == to || self.character(from).is_none() || self.character(to).is_none() {
            return false;
        }
        self.relationships.push(Relationship {
            id: Id::generate(),
            from: from.clone(),
            to: to.clone(),
            label: label.trim().to_owned(),
        });
        true
    }

    pub fn has_relationship(&self, from: &Id, to: &Id) -> bool {
        self.relationships
            .iter()
            .any(|r| &r.from == from && &r.to == to)
    }

    /// Adds an unlabelled relationship pointing the other way, since the
    /// reverse of a label ("pimp" → "in love") can't be inferred. Returns
    /// `false` when that reverse already exists or `id` is unknown.
    pub fn add_reverse_relationship(&mut self, id: &Id) -> bool {
        let Some(relationship) = self.relationships.iter().find(|r| &r.id == id) else {
            return false;
        };
        let (from, to) = (relationship.to.clone(), relationship.from.clone());
        !self.has_relationship(&from, &to) && self.add_relationship(&from, &to, "")
    }

    pub fn delete_relationship(&mut self, id: &Id) {
        self.relationships.retain(|r| &r.id != id);
    }

    pub fn relationships_of<'a>(&'a self, id: &'a Id) -> impl Iterator<Item = &'a Relationship> {
        self.relationships
            .iter()
            .filter(move |r| &r.from == id || &r.to == id)
    }

    /// Words of prose across every scene that is not archived.
    pub fn manuscript_word_count(&self) -> usize {
        self.manuscript_scenes().map(|(_, s)| s.word_count()).sum()
    }

    /// Indices of dated, non-archived scenes in story order; scenes on the
    /// same day keep their board order.
    pub fn chronological_indices(&self) -> Vec<usize> {
        let days = self.story_days();
        let mut dated: Vec<(StoryDay, usize)> = self
            .manuscript_scenes()
            .filter_map(|(i, _)| days[i].map(|day| (day, i)))
            .collect();
        dated.sort_unstable();
        dated.into_iter().map(|(_, i)| i).collect()
    }

    /// Each scene's place in the story's chronology, by board index; `None`
    /// for scenes without a (shown) day or date.
    pub fn story_days(&self) -> Vec<Option<StoryDay>> {
        if !self.calendar {
            return self
                .scenes
                .iter()
                .map(|s| {
                    s.shown_day().map(|number| StoryDay {
                        number: Some(number),
                        date: None,
                    })
                })
                .collect();
        }
        let days = self.calendar_days(None);
        self.scenes
            .iter()
            .map(|s| {
                let date = s.shown_date()?;
                Some(StoryDay {
                    number: (s.status != Status::Archived)
                        .then(|| days.number_of(date))
                        .flatten(),
                    date: Some(date),
                })
            })
            .collect()
    }

    /// Day numbering of the dated manuscript scenes, optionally leaving one
    /// scene out so it can be renumbered against the others.
    pub fn calendar_days(&self, excluding: Option<usize>) -> CalendarDays {
        let mut dates: Vec<StoryDate> = self
            .manuscript_scenes()
            .filter(|(i, _)| Some(*i) != excluding)
            .filter_map(|(_, s)| s.shown_date())
            .collect();
        dates.sort_unstable();
        dates.dedup();
        CalendarDays {
            dates,
            absolute: self.absolute_days,
        }
    }

    pub fn character(&self, id: &Id) -> Option<&Character> {
        self.characters.iter().find(|c| &c.id == id)
    }

    pub fn character_mut(&mut self, id: &Id) -> Option<&mut Character> {
        self.characters.iter_mut().find(|c| &c.id == id)
    }

    pub fn pov_scene_count(&self, id: &Id) -> usize {
        self.scenes
            .iter()
            .filter(|s| s.pov.as_ref() == Some(id))
            .count()
    }

    /// Per-status scene counts, zero-filled for statuses with no scenes.
    pub fn status_counts(&self) -> Vec<(Status, usize)> {
        Status::ALL
            .iter()
            .map(|&status| {
                (
                    status,
                    self.scenes.iter().filter(|s| s.status == status).count(),
                )
            })
            .collect()
    }

    /// Per-act scene counts, zero-filled for acts with no scenes.
    pub fn act_counts(&self) -> Vec<(Act, usize)> {
        Act::ALL
            .iter()
            .map(|&act| {
                (
                    act,
                    self.scenes.iter().filter(|s| s.act == Some(act)).count(),
                )
            })
            .collect()
    }

    fn manuscript_scenes(&self) -> impl Iterator<Item = (usize, &Scene)> {
        self.scenes
            .iter()
            .enumerate()
            .filter(|(_, s)| s.status != Status::Archived)
    }

    /// Manuscript words per act, zero-filled, with scenes lacking an act
    /// last under `None`.
    pub fn act_word_counts(&self) -> Vec<(Option<Act>, usize)> {
        Act::ALL
            .iter()
            .copied()
            .map(Some)
            .chain([None])
            .map(|act| {
                let words = self
                    .manuscript_scenes()
                    .filter(|(_, s)| s.act == act)
                    .map(|(_, s)| s.word_count())
                    .sum();
                (act, words)
            })
            .collect()
    }

    /// Manuscript words per POV character, most first; characters without
    /// a manuscript POV scene are left out.
    pub fn pov_word_counts(&self) -> Vec<(&Character, usize)> {
        let mut counts: Vec<(&Character, usize)> = self
            .characters
            .iter()
            .filter_map(|character| {
                let mut scenes = self
                    .manuscript_scenes()
                    .filter(|(_, s)| s.pov.as_ref() == Some(&character.id))
                    .peekable();
                scenes.peek()?;
                Some((character, scenes.map(|(_, s)| s.word_count()).sum()))
            })
            .collect();
        counts.sort_by_key(|&(_, words)| std::cmp::Reverse(words));
        counts
    }

    /// Manuscript scenes each character appears in, in board order: as POV,
    /// or by a name part no other character shares, written as a whole word
    /// in the title, summary or prose.
    pub fn character_appearances(&self) -> Vec<(&Character, Vec<usize>)> {
        let name_parts: Vec<HashSet<String>> = self
            .characters
            .iter()
            .map(|c| distinctive_name_parts(&c.name))
            .collect();
        let mut owners: HashMap<&str, usize> = HashMap::new();
        for part in name_parts.iter().flatten() {
            *owners.entry(part).or_default() += 1;
        }
        let scene_words: Vec<(usize, &Scene, HashSet<&str>)> = self
            .manuscript_scenes()
            .map(|(i, s)| {
                let words = [&s.title, &s.summary, &s.text]
                    .into_iter()
                    .flat_map(|field| words_of(field))
                    .collect();
                (i, s, words)
            })
            .collect();

        self.characters
            .iter()
            .zip(&name_parts)
            .map(|(character, parts)| {
                let unique: Vec<&str> = parts
                    .iter()
                    .map(String::as_str)
                    .filter(|part| owners[part] == 1)
                    .collect();
                let scenes = scene_words
                    .iter()
                    .filter(|(_, scene, words)| {
                        scene.pov.as_ref() == Some(&character.id)
                            || unique.iter().any(|part| words.contains(part))
                    })
                    .map(|&(i, _, _)| i)
                    .collect();
                (character, scenes)
            })
            .collect()
    }

    /// Each scene's chapter, by board index. Chapters split the manuscript
    /// scenes at chapter breaks, and once there is one, the first scene opens
    /// chapter 1 so no prose falls outside a chapter. Archived scenes, and
    /// every scene on a board without breaks, have none.
    pub fn chapters(&self) -> Vec<Option<ChapterPlace>> {
        let mut places = vec![None; self.scenes.len()];
        if !self.manuscript_scenes().any(|(_, s)| s.chapter_break) {
            return places;
        }
        let mut number = 0;
        for (index, scene) in self.manuscript_scenes() {
            let starts = number == 0 || scene.chapter_break;
            if starts {
                number += 1;
            }
            places[index] = Some(ChapterPlace { number, starts });
        }
        places
    }

    pub fn chapter_count(&self) -> usize {
        self.chapters()
            .into_iter()
            .flatten()
            .map(|place| place.number)
            .max()
            .unwrap_or(0)
    }

    pub fn scene_gaps(&self) -> SceneGaps {
        let indices_where = |missing: fn(&Scene) -> bool| {
            self.manuscript_scenes()
                .filter(|(_, s)| missing(s))
                .map(|(i, _)| i)
                .collect()
        };
        let days = self.story_days();
        SceneGaps {
            pov: indices_where(|s| s.pov.is_none()),
            location: indices_where(|s| s.location.is_none()),
            day: self
                .manuscript_scenes()
                .filter(|(i, _)| days[*i].is_none())
                .map(|(i, _)| i)
                .collect(),
        }
    }

    /// Indices of scenes matching the text query and optional
    /// status/act/pov filters. `None` filters match anything.
    pub fn matching_indices(
        &self,
        query: &str,
        status: Option<Status>,
        act: Option<Act>,
        pov: Option<&Id>,
    ) -> Vec<usize> {
        let needle = query.trim().to_lowercase();
        self.scenes
            .iter()
            .enumerate()
            .filter(|(_, scene)| {
                if let Some(status) = status
                    && scene.status != status
                {
                    return false;
                }
                if let Some(act) = act
                    && scene.act != Some(act)
                {
                    return false;
                }
                if let Some(pov) = pov
                    && scene.pov.as_ref() != Some(pov)
                {
                    return false;
                }
                if !needle.is_empty() {
                    let pov_name = scene
                        .pov
                        .as_ref()
                        .and_then(|id| self.character(id))
                        .map_or("", |c| c.name.as_str());
                    let location_name = scene
                        .location
                        .as_ref()
                        .and_then(|id| self.location(id))
                        .map_or("", |l| l.name.as_str());
                    let matches_field = contains_case_insensitive(&scene.title, &needle)
                        || contains_case_insensitive(&scene.summary, &needle)
                        || contains_case_insensitive(status_str(scene.status), &needle)
                        || scene
                            .act
                            .is_some_and(|a| contains_case_insensitive(act_str(a), &needle))
                        || contains_case_insensitive(pov_name, &needle)
                        || contains_case_insensitive(location_name, &needle)
                        || contains_case_insensitive(&scene.text, &needle);
                    if !matches_field {
                        return false;
                    }
                }
                true
            })
            .map(|(i, _)| i)
            .collect()
    }
}

fn shift_by<T>(items: &mut Vec<T>, is_target: impl Fn(&T) -> bool, offset: isize) -> bool {
    let Some(from) = items.iter().position(is_target) else {
        return false;
    };
    let Some(to) = from
        .checked_add_signed(offset)
        .filter(|&to| to < items.len() && to != from)
    else {
        return false;
    };
    let item = items.remove(from);
    items.insert(to, item);
    true
}

const fn status_str(status: Status) -> &'static str {
    match status {
        Status::Outline => "Outline",
        Status::Draft => "Draft",
        Status::Revision => "Revision",
        Status::Final => "Final",
        Status::Archived => "Archived",
    }
}

const fn act_str(act: Act) -> &'static str {
    match act {
        Act::One => "Act I",
        Act::Two => "Act II",
        Act::Three => "Act III",
    }
}

const HONORIFICS: [&str; 22] = [
    "mr",
    "mrs",
    "ms",
    "miss",
    "dr",
    "sir",
    "dame",
    "lady",
    "lord",
    "captain",
    "professor",
    "king",
    "queen",
    "prince",
    "princess",
    "aunt",
    "uncle",
    "saint",
    "st",
    "the",
    "of",
    "von",
];

fn words_of(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
}

/// Name parts worth searching for, capitalized because prose capitalizes
/// names and a lowercase match ("will", "hope") is usually not the name.
fn distinctive_name_parts(name: &str) -> HashSet<String> {
    words_of(name)
        .filter(|part| part.chars().count() >= 2)
        .filter(|part| !HONORIFICS.contains(&part.to_lowercase().as_str()))
        .filter_map(|part| {
            let mut chars = part.chars();
            let first = chars.next()?;
            Some(first.to_uppercase().chain(chars).collect())
        })
        .collect()
}

fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.is_ascii() && needle.is_ascii() {
        let needle_bytes = needle.as_bytes();
        haystack
            .as_bytes()
            .windows(needle_bytes.len())
            .any(|window| window.eq_ignore_ascii_case(needle_bytes))
    } else {
        haystack.to_lowercase().contains(needle)
    }
}

/// The HTML planner encoded "no value" as `""`; keep reading and writing that.
mod empty_string_as_none {
    use super::{Deserialize, Deserializer, Serialize, Serializer};
    use serde::de::{DeserializeOwned, IntoDeserializer, value::StringDeserializer};

    #[allow(
        clippy::ref_option,
        reason = "signature dictated by serde's `with` attribute"
    )]
    pub fn serialize<T: Serialize, S: Serializer>(
        value: &Option<T>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(v) => v.serialize(serializer),
            None => serializer.serialize_str(""),
        }
    }

    pub fn deserialize<'de, T: DeserializeOwned, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<T>, D::Error> {
        let raw = Option::<String>::deserialize(deserializer)?.unwrap_or_default();
        if raw.is_empty() {
            return Ok(None);
        }
        let inner: StringDeserializer<D::Error> = raw.into_deserializer();
        T::deserialize(inner).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_board_without_content_is_empty() {
        let mut board = Board {
            title: "Dune".to_owned(),
            ..Board::default()
        };
        assert!(board.is_empty());
        board.add_scene();
        assert!(!board.is_empty());
    }

    const HTML_EXPORT: &str = r#"{
      "title": "My Book",
      "characters": [{"id": "abc1234", "name": "Ada", "notes": ""}],
      "scenes": [
        {"id": "s1", "title": "Rain", "summary": "", "status": "Draft", "pov": "abc1234", "act": "Act II"},
        {"id": "s2", "title": "", "summary": "", "status": "Outline", "pov": "", "act": ""}
      ],
      "next": 0
    }"#;

    #[test]
    fn reads_html_planner_export() {
        let board: Board = serde_json::from_str(HTML_EXPORT).unwrap();
        assert_eq!(board.scenes[0].status, Status::Draft);
        assert_eq!(board.scenes[0].act, Some(Act::Two));
        assert_eq!(board.scenes[0].pov, Some(Id("abc1234".into())));
        assert_eq!(board.scenes[1].pov, None);
        assert_eq!(board.scenes[1].act, None);
    }

    #[test]
    fn round_trips_through_json() {
        let board: Board = serde_json::from_str(HTML_EXPORT).unwrap();
        let json = serde_json::to_string(&board).unwrap();
        assert!(json.contains(r#""act":"""#));
        assert_eq!(serde_json::from_str::<Board>(&json).unwrap(), board);
    }

    #[test]
    fn rejects_unknown_act() {
        let json = r#"{"scenes":[{"id":"x","act":"Act IV"}]}"#;
        assert!(serde_json::from_str::<Board>(json).is_err());
    }

    #[test]
    fn rejects_file_without_scenes() {
        assert!(serde_json::from_str::<Board>(r#"{"title":"x"}"#).is_err());
    }

    #[test]
    fn html_export_without_new_fields_gets_empty_defaults() {
        let board: Board = serde_json::from_str(HTML_EXPORT).unwrap();
        assert_eq!(board.locations, vec![]);
        assert_eq!(board.relationships, vec![]);
        assert_eq!(board.scenes[0].location, None);
        assert_eq!(board.scenes[0].day, None);
        assert_eq!(board.scenes[0].text, "");
        assert_eq!(board.characters[0].role, "");
        assert_eq!(board.characters[0].gender, "");
        let json = serde_json::to_string(&board).unwrap();
        assert!(!json.contains("\"day\""));
    }

    #[test]
    fn deleting_location_clears_scene_location() {
        let mut board = Board::default();
        assert!(board.add_location(" Harbor "));
        assert!(!board.add_location("  "));
        let harbor = board.locations[0].id.clone();
        assert_eq!(board.locations[0].name, "Harbor");
        let scene = board.add_scene();
        board.scene_mut(&scene).unwrap().location = Some(harbor.clone());
        assert_eq!(board.location_scene_count(&harbor), 1);
        assert_eq!(board.matching_indices("harb", None, None, None), vec![0]);

        board.delete_location(&harbor);
        assert_eq!(board.locations, vec![]);
        assert_eq!(board.scenes[0].location, None);
    }

    #[test]
    fn relationships_reject_self_links_and_die_with_their_characters() {
        let mut board = Board::default();
        board.add_character("Ada");
        board.add_character("Bo");
        board.add_character("Cy");
        let [ada, bo, cy] = [0, 1, 2].map(|i| board.characters[i].id.clone());

        assert!(!board.add_relationship(&ada, &ada, "self"));
        assert!(!board.add_relationship(&ada, &Id("missing".into()), "x"));
        assert!(board.add_relationship(&ada, &bo, " mentor "));
        assert!(board.add_relationship(&cy, &bo, "rival"));
        assert_eq!(board.relationships[0].label, "mentor");
        assert_eq!(board.relationships_of(&bo).count(), 2);

        board.delete_character(&ada);
        assert_eq!(board.relationships_of(&bo).count(), 1);
        let rival = board.relationships[0].id.clone();
        board.delete_relationship(&rival);
        assert_eq!(board.relationships, vec![]);
    }

    #[test]
    fn reverse_relationship_is_added_once_and_unlabelled() {
        let mut board = Board::default();
        board.add_character("Ada");
        board.add_character("Bo");
        let [ada, bo] = [0, 1].map(|i| board.characters[i].id.clone());
        board.add_relationship(&ada, &bo, "pimp");
        let pimp = board.relationships[0].id.clone();

        assert!(board.add_reverse_relationship(&pimp));
        assert!(!board.add_reverse_relationship(&pimp));
        assert!(!board.add_reverse_relationship(&Id("missing".into())));
        assert_eq!(board.relationships.len(), 2);
        let reverse = &board.relationships[1];
        assert_eq!((&reverse.from, &reverse.to), (&bo, &ada));
        assert_eq!(reverse.label, "");
    }

    #[test]
    fn manuscript_word_count_skips_archived_scenes() {
        let mut board = Board::default();
        let kept = board.add_scene();
        board.scene_mut(&kept).unwrap().text = "It was  a dark\nnight.".into();
        let cut = board.add_scene();
        let scene = board.scene_mut(&cut).unwrap();
        scene.text = "cut words here".into();
        scene.status = Status::Archived;
        assert_eq!(board.scenes[0].word_count(), 5);
        assert_eq!(board.manuscript_word_count(), 5);
        assert_eq!(board.matching_indices("dark", None, None, None), vec![0]);
    }

    #[test]
    fn chronological_order_sorts_by_day_then_board_order() {
        let mut board = Board::default();
        let days = [Some(3), None, Some(1), Some(3), Some(-2)];
        for day in days {
            let id = board.add_scene();
            board.scene_mut(&id).unwrap().day = day;
        }
        board.scenes[4].status = Status::Archived;
        assert_eq!(board.chronological_indices(), vec![2, 0, 3]);
    }

    #[test]
    fn deleting_character_clears_pov() {
        let mut board = Board::default();
        assert!(board.add_character("  Ada "));
        let ada = board.characters[0].id.clone();
        assert_eq!(board.characters[0].name, "Ada");
        let scene = board.add_scene();
        board.scene_mut(&scene).unwrap().pov = Some(ada.clone());
        assert_eq!(board.pov_scene_count(&ada), 1);

        board.delete_character(&ada);
        assert_eq!(board.characters, vec![]);
        assert_eq!(board.scenes[0].pov, None);
    }

    #[test]
    fn ignores_blank_character_name() {
        let mut board = Board::default();
        assert!(!board.add_character("   "));
        assert_eq!(board.characters, vec![]);
    }

    #[test]
    fn moves_scene_and_ignores_out_of_range() {
        let mut board = Board::default();
        let ids: Vec<_> = (0..3).map(|_| board.add_scene()).collect();
        board.move_scene(0, 2);
        assert_eq!(board.scenes[2].id, ids[0]);
        board.move_scene(0, 9);
        assert_eq!(board.scenes.len(), 3);
    }

    #[test]
    fn matching_indices_respect_filters() {
        let mut board = Board::default();
        let draft = board.add_scene();
        board.scene_mut(&draft).unwrap().status = Status::Draft;
        board.scene_mut(&draft).unwrap().act = Some(Act::One);
        board.scene_mut(&draft).unwrap().title = "Rain".into();
        let dawn = board.add_scene();
        board.scene_mut(&dawn).unwrap().status = Status::Final;
        board.scene_mut(&dawn).unwrap().title = "Dawn".into();

        assert_eq!(board.matching_indices("rain", None, None, None), vec![0]);
        assert_eq!(
            board.matching_indices("", Some(Status::Draft), None, None),
            vec![0]
        );
        assert_eq!(
            board.matching_indices("", None, Some(Act::One), None),
            vec![0]
        );
        assert_eq!(
            board.matching_indices("zzz", None, None, None),
            Vec::<usize>::new()
        );
        assert_eq!(board.matching_indices("", None, None, None), vec![0, 1]);
    }

    #[test]
    fn duplicate_scene_lands_after_original_with_new_id() {
        let mut board = Board::default();
        let first = board.add_scene();
        let last = board.add_scene();
        let scene = board.scene_mut(&first).unwrap();
        scene.title = "Rain".into();
        scene.status = Status::Draft;

        let copy = board.duplicate_scene(&first).unwrap();
        assert_ne!(copy, first);
        assert_eq!(board.scenes[1].id, copy);
        assert_eq!(board.scenes[1].title, "Rain (copy)");
        assert_eq!(board.scenes[1].status, Status::Draft);
        assert_eq!(board.scenes[2].id, last);

        let untitled = board.duplicate_scene(&last).unwrap();
        assert_eq!(board.scenes[3].id, untitled);
        assert_eq!(board.scenes[3].title, "");
        assert_eq!(board.duplicate_scene(&Id("missing".into())), None);
    }

    #[test]
    fn shift_scene_stops_at_board_edges() {
        let mut board = Board::default();
        let ids: Vec<_> = (0..3).map(|_| board.add_scene()).collect();

        assert!(!board.shift_scene(&ids[0], -1));
        assert!(board.shift_scene(&ids[0], 1));
        assert_eq!(board.scenes[1].id, ids[0]);
        assert!(board.shift_scene(&ids[0], 1));
        assert!(!board.shift_scene(&ids[0], 1));
        assert_eq!(board.scenes[2].id, ids[0]);
        assert!(board.shift_scene(&ids[0], -1));
        assert_eq!(board.scenes[1].id, ids[0]);
        assert!(!board.shift_scene(&Id("missing".into()), 1));
    }

    #[test]
    fn search_matches_pov_name_status_and_act_case_insensitively() {
        let mut board = Board::default();
        board.add_character("Ada Lovelace");
        let ada = board.characters[0].id.clone();
        let rain = board.add_scene();
        let scene = board.scene_mut(&rain).unwrap();
        scene.pov = Some(ada);
        scene.status = Status::Draft;
        scene.act = Some(Act::Two);
        board.add_scene();

        assert_eq!(
            board.matching_indices("lovelace", None, None, None),
            vec![0]
        );
        assert_eq!(board.matching_indices("DRAFT", None, None, None), vec![0]);
        assert_eq!(board.matching_indices("act ii", None, None, None), vec![0]);
    }

    #[test]
    fn search_matches_non_ascii_text_and_not_across_fields() {
        let mut board = Board::default();
        let id = board.add_scene();
        let scene = board.scene_mut(&id).unwrap();
        scene.title = "Ärger im Café".into();
        scene.summary = "Rain".into();
        board.add_scene();

        assert_eq!(board.matching_indices("ärger", None, None, None), vec![0]);
        assert_eq!(board.matching_indices("CAFÉ", None, None, None), vec![0]);
        assert_eq!(
            board.matching_indices("café rain", None, None, None),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn case_insensitive_contains_handles_short_haystacks_and_changing_widths() {
        assert!(contains_case_insensitive("anything", ""));
        assert!(!contains_case_insensitive("ab", "abc"));
        assert!(contains_case_insensitive("ἈΘΉΝΑ", "ἀθήνα"));
        assert!(contains_case_insensitive(
            "İstanbul",
            &"İstanbul".to_lowercase()
        ));
    }

    #[test]
    fn shift_character_stops_at_list_edges() {
        let mut board = Board::default();
        for name in ["Ada", "Bo", "Cy"] {
            board.add_character(name);
        }
        let cy = board.characters[2].id.clone();

        assert!(!board.shift_character(&cy, 1));
        assert!(board.shift_character(&cy, -1));
        assert!(board.shift_character(&cy, -1));
        assert!(!board.shift_character(&cy, -1));
        assert_eq!(board.character_index(&cy), Some(0));
        let names: Vec<_> = board.characters.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Cy", "Ada", "Bo"]);
    }

    #[test]
    fn unset_pov_filter_keeps_scenes_with_a_pov() {
        let mut board = Board::default();
        board.add_character("Ada");
        let ada = board.characters[0].id.clone();
        let with_pov = board.add_scene();
        board.scene_mut(&with_pov).unwrap().pov = Some(ada.clone());
        board.add_scene();

        assert_eq!(board.matching_indices("", None, None, None), vec![0, 1]);
        assert_eq!(board.matching_indices("", None, None, Some(&ada)), vec![0]);
    }

    #[test]
    fn status_and_act_counts_are_zero_filled() {
        let mut board = Board::default();
        board.add_scene();
        board.add_scene();
        let counts = board.status_counts();
        assert_eq!(
            counts,
            vec![
                (Status::Outline, 2),
                (Status::Draft, 0),
                (Status::Revision, 0),
                (Status::Final, 0),
                (Status::Archived, 0)
            ]
        );
        let draft = board.add_scene();
        board.scene_mut(&draft).unwrap().status = Status::Draft;
        board.scene_mut(&draft).unwrap().act = Some(Act::Three);
        assert_eq!(
            board.status_counts(),
            vec![
                (Status::Outline, 2),
                (Status::Draft, 1),
                (Status::Revision, 0),
                (Status::Final, 0),
                (Status::Archived, 0)
            ]
        );
        assert_eq!(
            board.act_counts(),
            vec![(Act::One, 0), (Act::Two, 0), (Act::Three, 1)]
        );
    }

    fn scene_with(board: &mut Board, text: &str, edit: impl FnOnce(&mut Scene)) {
        let id = board.add_scene();
        let scene = board.scene_mut(&id).unwrap();
        text.clone_into(&mut scene.text);
        edit(scene);
    }

    #[test]
    fn act_word_counts_skip_archived_and_bucket_unassigned_last() {
        let mut board = Board::default();
        scene_with(&mut board, "one two three", |s| s.act = Some(Act::One));
        scene_with(&mut board, "four five", |s| s.act = Some(Act::One));
        scene_with(&mut board, "six", |_| {});
        scene_with(&mut board, "cut words here", |s| {
            s.act = Some(Act::Two);
            s.status = Status::Archived;
        });

        assert_eq!(
            board.act_word_counts(),
            vec![
                (Some(Act::One), 5),
                (Some(Act::Two), 0),
                (Some(Act::Three), 0),
                (None, 1)
            ]
        );
    }

    #[test]
    fn pov_word_counts_rank_by_words_and_skip_characters_without_pov_scenes() {
        let mut board = Board::default();
        for name in ["Ada", "Bo", "Cy", "Di"] {
            board.add_character(name);
        }
        let id = |board: &Board, n: usize| board.characters[n].id.clone();
        let (ada, bo, cy, di) = (id(&board, 0), id(&board, 1), id(&board, 2), id(&board, 3));
        scene_with(&mut board, "a b", |s| s.pov = Some(ada));
        scene_with(&mut board, "a b c", |s| s.pov = Some(bo.clone()));
        scene_with(&mut board, "", |s| s.pov = Some(bo));
        scene_with(&mut board, "", |s| s.pov = Some(cy.clone()));
        scene_with(&mut board, "archived only", |s| {
            s.pov = Some(di);
            s.status = Status::Archived;
        });

        let names: Vec<(&str, usize)> = board
            .pov_word_counts()
            .into_iter()
            .map(|(c, words)| (c.name.as_str(), words))
            .collect();
        assert_eq!(names, vec![("Bo", 3), ("Ada", 2), ("Cy", 0)]);
    }

    #[test]
    fn scene_gaps_list_manuscript_scenes_missing_each_detail() {
        let mut board = Board::default();
        board.add_character("Ada");
        board.add_location("Harbor");
        let ada = board.characters[0].id.clone();
        let harbor = board.locations[0].id.clone();
        scene_with(&mut board, "", |s| {
            s.pov = Some(ada.clone());
            s.location = Some(harbor.clone());
            s.day = Some(1);
        });
        scene_with(&mut board, "", |s| s.pov = Some(ada));
        scene_with(&mut board, "", |s| s.day = Some(2));
        scene_with(&mut board, "", |s| s.status = Status::Archived);

        assert_eq!(
            board.scene_gaps(),
            SceneGaps {
                pov: vec![2],
                location: vec![1, 2],
                day: vec![1],
            }
        );
    }

    fn appearance_names(board: &Board) -> Vec<(&str, Vec<usize>)> {
        board
            .character_appearances()
            .into_iter()
            .map(|(c, scenes)| (c.name.as_str(), scenes))
            .collect()
    }

    #[test]
    fn appearances_match_whole_capitalized_name_parts_and_pov() {
        let mut board = Board::default();
        board.add_character("Captain Will Ahab");
        board.add_character("ada");
        let ada = board.characters[1].id.clone();
        scene_with(&mut board, "Ahab's leg ached.", |_| {});
        scene_with(&mut board, "You will see. Captain or not.", |_| {});
        scene_with(&mut board, "", |s| s.pov = Some(ada));
        scene_with(&mut board, "Adamant, Ada left.", |_| {});
        scene_with(&mut board, "", |s| s.summary = "Will meets Ada".to_owned());
        scene_with(&mut board, "Ahab again", |s| s.status = Status::Archived);

        assert_eq!(
            appearance_names(&board),
            vec![("Captain Will Ahab", vec![0, 4]), ("ada", vec![2, 3, 4])]
        );
    }

    #[test]
    fn shared_name_parts_do_not_count_as_appearances() {
        let mut board = Board::default();
        board.add_character("Jane Bennet");
        board.add_character("Elizabeth Bennet");
        board.add_character("Bennet");
        scene_with(&mut board, "The Bennet house", |_| {});
        scene_with(&mut board, "Elizabeth laughed", |_| {});

        assert_eq!(
            appearance_names(&board),
            vec![
                ("Jane Bennet", vec![]),
                ("Elizabeth Bennet", vec![1]),
                ("Bennet", vec![])
            ]
        );
    }

    #[test]
    fn word_goal_is_saved_only_when_set() {
        let mut board = Board::default();
        let json = serde_json::to_string(&board).unwrap();
        assert!(!json.contains("wordGoal"));
        assert_eq!(
            serde_json::from_str::<Board>(&json).unwrap().word_goal,
            None
        );

        board.word_goal = Some(80_000);
        let json = serde_json::to_string(&board).unwrap();
        assert!(json.contains(r#""wordGoal":80000"#));
        assert_eq!(serde_json::from_str::<Board>(&json).unwrap(), board);
    }

    #[test]
    fn chapters_split_manuscript_scenes_at_breaks() {
        let mut board = Board::default();
        scene_with(&mut board, "", |_| {});
        scene_with(&mut board, "", |_| {});
        scene_with(&mut board, "", |s| s.chapter_break = true);
        scene_with(&mut board, "", |s| {
            s.chapter_break = true;
            s.status = Status::Archived;
        });
        scene_with(&mut board, "", |_| {});
        scene_with(&mut board, "", |s| s.chapter_break = true);

        let place = |number, starts| Some(ChapterPlace { number, starts });
        assert_eq!(
            board.chapters(),
            vec![
                place(1, true),
                place(1, false),
                place(2, true),
                None,
                place(2, false),
                place(3, true)
            ]
        );
        assert_eq!(board.chapter_count(), 3);
    }

    #[test]
    fn boards_without_live_breaks_have_no_chapters() {
        let mut board = Board::default();
        scene_with(&mut board, "", |_| {});
        scene_with(&mut board, "", |s| {
            s.chapter_break = true;
            s.status = Status::Archived;
        });

        assert_eq!(board.chapters(), vec![None, None]);
        assert_eq!(board.chapter_count(), 0);
    }

    #[test]
    fn chapter_break_is_saved_only_when_set_and_not_duplicated() {
        let mut board = Board::default();
        scene_with(&mut board, "", |_| {});
        let json = serde_json::to_string(&board).unwrap();
        assert!(!json.contains("chapterBreak"));

        board.scenes[0].chapter_break = true;
        let json = serde_json::to_string(&board).unwrap();
        assert!(json.contains(r#""chapterBreak":true"#));
        assert_eq!(serde_json::from_str::<Board>(&json).unwrap(), board);

        let original = board.scenes[0].id.clone();
        board.duplicate_scene(&original);
        assert!(!board.scenes[1].chapter_break);
    }

    fn date(text: &str) -> StoryDate {
        text.parse().unwrap()
    }

    fn day_labels(board: &Board) -> Vec<String> {
        board
            .story_days()
            .into_iter()
            .map(|day| day.map_or_default(StoryDay::label))
            .collect()
    }

    fn calendar_board(dates: &[&str]) -> Board {
        let mut board = Board {
            calendar: true,
            ..Board::default()
        };
        for text in dates {
            scene_with(&mut board, "", |s| s.date = Some(date(text)));
        }
        board
    }

    #[test]
    fn calendar_counts_story_days_from_the_earliest_date() {
        let board = calendar_board(&["2026-03-20", "2026-03-15", "2026-03-20", "2026-03-16"]);

        assert_eq!(
            day_labels(&board),
            [
                "Day 3 · Fri 20 Mar 2026",
                "Day 1 · Sun 15 Mar 2026",
                "Day 3 · Fri 20 Mar 2026",
                "Day 2 · Mon 16 Mar 2026",
            ]
        );
        assert_eq!(board.chronological_indices(), [1, 3, 0, 2]);
        assert_eq!(board.calendar_days(None).first(), Some(date("2026-03-15")));
    }

    #[test]
    fn day_numbers_map_back_to_dates_against_the_other_scenes() {
        let mut board = calendar_board(&["2026-03-01", "2026-03-05", "2026-03-09"]);
        let others = board.calendar_days(Some(0));
        assert_eq!(others.date_of(1), Some(date("2026-03-05")));
        assert_eq!(others.date_of(2), Some(date("2026-03-09")));
        assert_eq!(others.date_of(3), Some(date("2026-03-10")));
        assert_eq!(others.date_of(0), Some(date("2026-03-04")));

        board.scenes[0].date = others.date_of(3);
        assert_eq!(board.story_days()[0].and_then(|d| d.number), Some(3));

        board.absolute_days = true;
        let others = board.calendar_days(Some(1));
        assert_eq!(others.date_of(3), Some(date("2026-03-11")));
        assert_eq!(others.date_of(-1), Some(date("2026-03-07")));
        assert_eq!(others.date_of(i32::MAX), None);
        assert_eq!(Board::default().calendar_days(None).date_of(1), None);
    }

    #[test]
    fn absolute_days_count_calendar_distance() {
        let mut board = calendar_board(&["2026-03-20", "2026-03-15", "2026-03-16"]);
        board.absolute_days = true;

        assert_eq!(
            day_labels(&board),
            [
                "Day 6 · Fri 20 Mar 2026",
                "Day 1 · Sun 15 Mar 2026",
                "Day 2 · Mon 16 Mar 2026",
            ]
        );
    }

    #[test]
    fn archived_and_hidden_scenes_do_not_move_day_one() {
        let mut board = calendar_board(&["2026-03-15", "2026-03-10", "2026-03-12"]);
        board.scenes[1].status = Status::Archived;
        board.scenes[2].day_hidden = true;

        assert_eq!(
            day_labels(&board),
            ["Day 1 · Sun 15 Mar 2026", "Tue 10 Mar 2026", ""]
        );
        assert_eq!(board.scene_gaps().day, [2]);
        assert_eq!(board.calendar_days(None).first(), Some(date("2026-03-15")));
    }

    #[test]
    fn hidden_day_and_date_are_kept_and_saved() {
        let mut board = calendar_board(&["2026-03-15"]);
        board.scenes[0].day = Some(4);
        board.scenes[0].day_hidden = true;
        assert_eq!(board.story_days(), [None]);

        let json = serde_json::to_string(&board).unwrap();
        assert!(json.contains(r#""day":4,"date":"2026-03-15","dayHidden":true"#));
        let loaded = serde_json::from_str::<Board>(&json).unwrap();
        assert_eq!(loaded, board);

        board.scenes[0].day_hidden = false;
        board.calendar = false;
        assert_eq!(day_labels(&board), ["Day 4"]);
    }

    #[test]
    fn calendar_settings_are_saved_only_when_set() {
        let mut board = Board::default();
        let json = serde_json::to_string(&board).unwrap();
        assert!(!json.contains("calendar") && !json.contains("absoluteDays"));

        board.calendar = true;
        board.absolute_days = true;
        let json = serde_json::to_string(&board).unwrap();
        assert!(json.contains(r#""calendar":true,"absoluteDays":true"#));
        assert_eq!(serde_json::from_str::<Board>(&json).unwrap(), board);
    }

    #[test]
    fn story_start_from_the_first_calendar_becomes_scene_dates() {
        let json = r#"{"scenes": [
            {"id": "a", "day": 3},
            {"id": "b", "day": 0},
            {"id": "c"}
        ], "storyStart": "2026-03-16"}"#;
        let board: Board = serde_json::from_str(json).unwrap();

        assert!(board.calendar);
        let dates: Vec<_> = board.scenes.iter().map(|s| s.date).collect();
        assert_eq!(
            dates,
            [Some(date("2026-03-18")), Some(date("2026-03-15")), None]
        );
        assert_eq!(board.scenes[0].day, Some(3));
        assert!(
            !serde_json::to_string(&board)
                .unwrap()
                .contains("storyStart")
        );

        let invalid = json.replace("2026-03-16", "2026-02-30");
        assert!(serde_json::from_str::<Board>(&invalid).is_err());
    }

    #[test]
    fn blank_and_default_titles_are_placeholders() {
        let titled = |title: &str| Board {
            title: title.to_owned(),
            ..Board::default()
        };
        assert!(Board::default().has_placeholder_title());
        assert!(titled("").has_placeholder_title());
        assert!(titled("  Untitled Novel ").has_placeholder_title());
        assert!(!titled("Dune").has_placeholder_title());
    }
}
