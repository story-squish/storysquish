use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

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
    /// In-story chronology, independent of the scene's place on the board.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day: Option<i32>,
    #[serde(default)]
    pub text: String,
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
            text: String::new(),
        }
    }

    pub fn word_count(&self) -> usize {
        self.text.split_whitespace().count()
    }
}

/// On-disk format matches the JSON exported by the original HTML planner,
/// so existing exports import unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
}

impl Default for Board {
    fn default() -> Self {
        Self {
            title: DEFAULT_TITLE.to_owned(),
            characters: Vec::new(),
            scenes: Vec::new(),
            locations: Vec::new(),
            relationships: Vec::new(),
        }
    }
}

impl Board {
    /// The title alone does not count: a new board keeps a default one.
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
        self.scenes
            .iter()
            .filter(|s| s.status != Status::Archived)
            .map(Scene::word_count)
            .sum()
    }

    /// Indices of dated, non-archived scenes in story order; scenes on the
    /// same day keep their board order.
    pub fn chronological_indices(&self) -> Vec<usize> {
        let mut dated: Vec<(i32, usize)> = self
            .scenes
            .iter()
            .enumerate()
            .filter(|(_, s)| s.status != Status::Archived)
            .filter_map(|(i, s)| s.day.map(|day| (day, i)))
            .collect();
        dated.sort_unstable();
        dated.into_iter().map(|(_, i)| i).collect()
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
                    let haystack = format!(
                        "{} {} {} {} {} {} {}",
                        scene.title,
                        scene.summary,
                        scene.status,
                        scene.act.map_or_else(String::new, |a| a.to_string()),
                        pov_name,
                        location_name,
                        scene.text,
                    )
                    .to_lowercase();
                    if !haystack.contains(&needle) {
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
