use std::str::FromStr;

use anyhow::{Result, bail};

use crate::calendar::StoryDate;
use crate::model::{Act, Board, Id, Status};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Column {
    Title,
    Summary,
    Status,
    Act,
    Pov,
    Location,
    Day,
    Date,
    Text,
}

impl Column {
    const ALL: [Self; 9] = [
        Self::Title,
        Self::Summary,
        Self::Status,
        Self::Act,
        Self::Pov,
        Self::Location,
        Self::Day,
        Self::Date,
        Self::Text,
    ];

    fn from_header(header: &str) -> Option<Self> {
        let key: String = header
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        Some(match key.as_str() {
            "title" | "scene" | "scenetitle" | "name" => Self::Title,
            "summary" | "beat" | "description" | "synopsis" | "scenesummary" => Self::Summary,
            "status" => Self::Status,
            "act" => Self::Act,
            "pov" | "povcharacter" | "viewpoint" => Self::Pov,
            "location" | "setting" | "place" => Self::Location,
            "day" | "storyday" => Self::Day,
            "date" | "storydate" => Self::Date,
            "text" | "prose" | "content" | "draft" => Self::Text,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::Summary => "Summary",
            Self::Status => "Status",
            Self::Act => "Act",
            Self::Pov => "POV",
            Self::Location => "Location",
            Self::Day => "Day",
            Self::Date => "Date",
            Self::Text => "Text",
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub scenes: usize,
    pub new_characters: usize,
    pub new_locations: usize,
    pub ignored_columns: Vec<String>,
}

#[derive(Default)]
struct ParsedScene {
    title: String,
    summary: String,
    status: Status,
    act: Option<Act>,
    pov: Option<String>,
    location: Option<String>,
    day: Option<i32>,
    date: Option<StoryDate>,
    text: String,
}

/// Adds one scene per row at the end of the board, in file order. Rows are
/// checked before anything is added, so a bad row leaves the board as it was.
/// Characters and locations are matched by name, ignoring case, and created
/// when the board has no such name.
pub fn append_scenes(board: &mut Board, csv: &str) -> Result<Summary> {
    let records = parse_records(csv)?;
    let Some((header, rows)) = records.split_first() else {
        bail!("the file is empty");
    };

    let mut columns: Vec<Option<Column>> = Vec::with_capacity(header.len());
    let mut ignored_columns = Vec::new();
    for name in header {
        let column = Column::from_header(name).filter(|column| !columns.contains(&Some(*column)));
        if column.is_none() && !name.trim().is_empty() {
            ignored_columns.push(name.trim().to_owned());
        }
        columns.push(column);
    }
    if columns.iter().all(Option::is_none) {
        let expected: Vec<&str> = Column::ALL.iter().map(|c| c.name()).collect();
        bail!(
            "the first row must name the columns, such as {}",
            expected.join(", ")
        );
    }

    let mut scenes = Vec::new();
    for (offset, row) in rows.iter().enumerate() {
        let row_number = offset + 2;
        let mut scene = ParsedScene::default();
        let mut has_content = false;
        for (column, value) in columns.iter().zip(row) {
            let Some(column) = column else { continue };
            let value = value.trim();
            has_content |= !value.is_empty();
            apply_value(&mut scene, *column, value).map_err(|reason| {
                anyhow::anyhow!("row {row_number}, {}: {reason}", column.name())
            })?;
        }
        if has_content {
            scenes.push(scene);
        }
    }
    if scenes.is_empty() {
        bail!("the file has no scene rows below the header");
    }

    let characters_before = board.characters.len();
    let locations_before = board.locations.len();
    let count = scenes.len();
    for parsed in scenes {
        let pov = parsed.pov.and_then(|name| character_named(board, &name));
        let location = parsed
            .location
            .and_then(|name| location_named(board, &name));
        let id = board.add_scene();
        if let Some(scene) = board.scene_mut(&id) {
            scene.title = parsed.title;
            scene.summary = parsed.summary;
            scene.status = parsed.status;
            scene.act = parsed.act;
            scene.pov = pov;
            scene.location = location;
            scene.day = parsed.day;
            scene.date = parsed.date;
            scene.text = parsed.text;
        }
    }
    Ok(Summary {
        scenes: count,
        new_characters: board.characters.len() - characters_before,
        new_locations: board.locations.len() - locations_before,
        ignored_columns,
    })
}

fn apply_value(scene: &mut ParsedScene, column: Column, value: &str) -> Result<(), String> {
    let name = (!value.is_empty()).then(|| value.to_owned());
    match column {
        Column::Title => value.clone_into(&mut scene.title),
        Column::Summary => value.clone_into(&mut scene.summary),
        Column::Text => value.clone_into(&mut scene.text),
        Column::Pov => scene.pov = name,
        Column::Location => scene.location = name,
        Column::Status => {
            if !value.is_empty() {
                scene.status = parse_status(value)?;
            }
        }
        Column::Act => scene.act = parse_act(value)?,
        Column::Day => {
            scene.day = if value.is_empty() {
                None
            } else {
                Some(
                    value
                        .parse()
                        .map_err(|_| format!("{value:?} is not a whole number"))?,
                )
            };
        }
        Column::Date => {
            scene.date = if value.is_empty() {
                None
            } else {
                Some(StoryDate::from_str(value).map_err(|e| format!("{value:?}: {e}"))?)
            };
        }
    }
    Ok(())
}

fn parse_status(value: &str) -> Result<Status, String> {
    Status::ALL
        .into_iter()
        .find(|status| status.to_string().eq_ignore_ascii_case(value))
        .ok_or_else(|| {
            let known: Vec<String> = Status::ALL.iter().map(ToString::to_string).collect();
            format!("unknown status {value:?}; use {}", known.join(", "))
        })
}

fn parse_act(value: &str) -> Result<Option<Act>, String> {
    let lowered = value.to_lowercase();
    let number = lowered.strip_prefix("act").unwrap_or(&lowered).trim();
    Ok(Some(match number {
        "" => return Ok(None),
        "i" | "1" | "one" => Act::One,
        "ii" | "2" | "two" => Act::Two,
        "iii" | "3" | "three" => Act::Three,
        _ => return Err(format!("unknown act {value:?}; use Act I, II or III")),
    }))
}

fn character_named(board: &mut Board, name: &str) -> Option<Id> {
    let existing = board
        .characters
        .iter()
        .find(|c| c.name.trim().eq_ignore_ascii_case(name));
    if let Some(character) = existing {
        return Some(character.id.clone());
    }
    board.add_character(name);
    board.characters.last().map(|c| c.id.clone())
}

fn location_named(board: &mut Board, name: &str) -> Option<Id> {
    let existing = board
        .locations
        .iter()
        .find(|l| l.name.trim().eq_ignore_ascii_case(name));
    if let Some(location) = existing {
        return Some(location.id.clone());
    }
    board.add_location(name);
    board.locations.last().map(|l| l.id.clone())
}

/// Splits RFC 4180 records on the delimiter the header row uses, which
/// spreadsheets in some locales set to `;` or a tab. Rows with no cell
/// content are dropped.
fn parse_records(text: &str) -> Result<Vec<Vec<String>>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let delimiter = detect_delimiter(text);
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, in_quotes) {
            ('"', true) => {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            }
            ('"', false) if field.is_empty() => in_quotes = true,
            (c, false) if c == delimiter => record.push(std::mem::take(&mut field)),
            ('\r' | '\n', false) => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                record.push(std::mem::take(&mut field));
                finish_record(&mut records, std::mem::take(&mut record));
            }
            (c, _) => field.push(c),
        }
    }
    if in_quotes {
        bail!("a quoted cell is never closed");
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        finish_record(&mut records, record);
    }
    Ok(records)
}

fn finish_record(records: &mut Vec<Vec<String>>, record: Vec<String>) {
    if record.iter().any(|cell| !cell.trim().is_empty()) {
        records.push(record);
    }
}

fn detect_delimiter(text: &str) -> char {
    let header = text.lines().next().unwrap_or_default();
    [',', ';', '\t']
        .into_iter()
        .max_by_key(|&d| header.matches(d).count())
        .filter(|&d| header.contains(d))
        .unwrap_or(',')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imported(csv: &str) -> (Board, Summary) {
        let mut board = Board::default();
        let summary = append_scenes(&mut board, csv).unwrap();
        (board, summary)
    }

    #[test]
    fn rows_become_scenes_in_file_order() {
        let (board, summary) = imported(
            "Title,Summary,Status,Act,Day\n\
             Opening,Ada arrives,Draft,Act I,1\n\
             Storm,\"Wind, rain\",Outline,II,3\n",
        );
        assert_eq!(summary.scenes, 2);
        let scenes = &board.scenes;
        assert_eq!(scenes[0].title, "Opening");
        assert_eq!(scenes[0].status, Status::Draft);
        assert_eq!(scenes[0].act, Some(Act::One));
        assert_eq!(scenes[0].day, Some(1));
        assert_eq!(scenes[1].summary, "Wind, rain");
        assert_eq!(scenes[1].act, Some(Act::Two));
    }

    #[test]
    fn quoted_cells_keep_newlines_and_escaped_quotes() {
        let (board, _) = imported("title,summary\r\n\"One\",\"She said \"\"go\"\"\nand left\"\r\n");
        assert_eq!(board.scenes[0].summary, "She said \"go\"\nand left");
    }

    #[test]
    fn semicolon_and_tab_delimiters_and_a_bom_are_understood() {
        let (board, _) = imported("\u{feff}Title;Summary\nA;first\n");
        assert_eq!(board.scenes[0].summary, "first");
        let (board, _) = imported("Title\tSummary\nB\tsecond\n");
        assert_eq!(board.scenes[0].title, "B");
        assert_eq!(board.scenes[0].summary, "second");
    }

    #[test]
    fn headers_ignore_case_spacing_and_common_synonyms() {
        let (board, _) =
            imported("SCENE,Beat,POV Character,Setting\nOpening,Ada waits,Ada,Harbor\n");
        let scene = &board.scenes[0];
        assert_eq!(scene.title, "Opening");
        assert_eq!(scene.summary, "Ada waits");
        assert_eq!(board.characters[0].name, "Ada");
        assert_eq!(board.locations[0].name, "Harbor");
    }

    #[test]
    fn people_and_places_are_matched_by_name_and_created_once() {
        let mut board = Board::default();
        board.add_character("Ada");
        board.add_location("Harbor");
        let summary = append_scenes(
            &mut board,
            "title,pov,location\nA,ada,harbor\nB,Bo,Castle\nC,BO,castle\n",
        )
        .unwrap();
        assert_eq!(board.characters.len(), 2);
        assert_eq!(board.locations.len(), 2);
        assert_eq!((summary.new_characters, summary.new_locations), (1, 1));
        assert_eq!(board.scenes[0].pov, Some(board.characters[0].id.clone()));
        assert_eq!(board.scenes[1].pov, board.scenes[2].pov);
        assert_eq!(board.scenes[1].location, board.scenes[2].location);
    }

    #[test]
    fn scenes_are_added_after_the_existing_ones() {
        let mut board = Board::default();
        let first = board.add_scene();
        append_scenes(&mut board, "title\nNew\n").unwrap();
        assert_eq!(board.scenes.len(), 2);
        assert_eq!(board.scenes[0].id, first);
        assert_eq!(board.scenes[1].title, "New");
    }

    #[test]
    fn a_bad_value_names_its_row_and_column_and_adds_nothing() {
        let mut board = Board::default();
        let error = append_scenes(&mut board, "title,status,pov\nA,Draft,Ada\nB,Drafty,Bo\n")
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("row 3, Status: unknown status \"Drafty\""),
            "{error}"
        );
        assert_eq!(board, Board::default());
    }

    #[test]
    fn acts_days_and_dates_are_checked() {
        let mut board = Board::default();
        for (csv, expected) in [
            ("act\nIV\n", "row 2, Act:"),
            ("day\nsoon\n", "row 2, Day: \"soon\" is not a whole number"),
            ("date\n2026-13-01\n", "row 2, Date:"),
        ] {
            let error = append_scenes(&mut board, csv).unwrap_err().to_string();
            assert!(error.starts_with(expected), "{error}");
        }
        let (board, _) = imported("act,date\nAct 3,2026-03-18\n3,\n");
        assert_eq!(board.scenes[0].act, Some(Act::Three));
        assert_eq!(board.scenes[0].date, Some("2026-03-18".parse().unwrap()));
        assert_eq!(board.scenes[1].act, Some(Act::Three));
        assert_eq!(board.scenes[1].date, None);
    }

    #[test]
    fn unrecognised_columns_are_reported_and_skipped() {
        let (board, summary) = imported("Title,Color,Notes\nA,red,hm\n");
        assert_eq!(board.scenes[0].title, "A");
        assert_eq!(summary.ignored_columns, vec!["Color", "Notes"]);
    }

    #[test]
    fn a_repeated_column_keeps_the_first_and_reports_the_rest() {
        let (board, summary) = imported("Title,Name\nFirst,Second\n");
        assert_eq!(board.scenes[0].title, "First");
        assert_eq!(summary.ignored_columns, vec!["Name"]);
    }

    #[test]
    fn blank_rows_are_skipped_and_short_rows_are_padded() {
        let (board, summary) = imported("title,summary,status\nA\n\n,,\nB,two\n");
        assert_eq!(summary.scenes, 2);
        assert_eq!(board.scenes[0].summary, "");
        assert_eq!(board.scenes[1].summary, "two");
        assert_eq!(board.scenes[1].status, Status::Outline);
    }

    #[test]
    fn files_that_are_not_scene_tables_are_refused() {
        let mut board = Board::default();
        for (csv, expected) in [
            ("", "empty"),
            (
                "Ada waits,Harbor\nBo leaves,Castle\n",
                "must name the columns",
            ),
            ("title,summary\n", "no scene rows"),
            ("title\n\"open\n", "never closed"),
        ] {
            let error = append_scenes(&mut board, csv).unwrap_err().to_string();
            assert!(error.contains(expected), "{csv:?}: {error}");
        }
        assert_eq!(board, Board::default());
    }
}
