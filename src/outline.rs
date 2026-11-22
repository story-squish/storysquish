use std::fmt::Write;

use crate::model::{Board, Status};

/// Archived scenes are cut material, so they stay out of the outline; the
/// remaining scenes keep their board numbers so they match the cards.
pub fn to_markdown(board: &Board) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# {}\n", novel_title(board));

    for (index, scene) in board.scenes.iter().enumerate() {
        if scene.status == Status::Archived {
            continue;
        }
        let scene_title = scene.title.trim();
        let _ = writeln!(
            out,
            "## Scene {:02}: {}\n",
            index + 1,
            if scene_title.is_empty() {
                "Untitled"
            } else {
                scene_title
            }
        );

        let mut facts = vec![scene.status.to_string()];
        if let Some(act) = scene.act {
            facts.push(act.to_string());
        }
        if let Some(pov) = scene.pov.as_ref().and_then(|id| board.character(id)) {
            facts.push(format!("POV: {}", pov.name.trim()));
        }
        if let Some(location) = scene.location.as_ref().and_then(|id| board.location(id)) {
            facts.push(format!("Location: {}", location.name.trim()));
        }
        if let Some(day) = scene.day {
            facts.push(format!("Day {day}"));
        }
        let _ = writeln!(out, "_{}_\n", facts.join(" · "));

        let summary = scene.summary.trim();
        if !summary.is_empty() {
            let _ = writeln!(out, "{summary}\n");
        }
    }

    if !board.characters.is_empty() {
        out.push_str("## Characters\n\n");
        for character in &board.characters {
            let _ = writeln!(out, "### {}\n", character.name.trim());
            for (field, value) in [("Role", &character.role), ("Goal", &character.goal)] {
                let value = value.trim();
                if !value.is_empty() {
                    let _ = writeln!(out, "**{field}:** {value}\n");
                }
            }
            for relationship in board
                .relationships
                .iter()
                .filter(|r| r.from == character.id)
            {
                let Some(other) = board.character(&relationship.to) else {
                    continue;
                };
                let label = relationship.label.trim();
                let label = if label.is_empty() {
                    "related to"
                } else {
                    label
                };
                let _ = writeln!(out, "- {label} → {}", other.name.trim());
            }
            if board.relationships.iter().any(|r| r.from == character.id) {
                out.push('\n');
            }
            let notes = character.notes.trim();
            if !notes.is_empty() {
                let _ = writeln!(out, "{notes}\n");
            }
        }
    }

    if !board.locations.is_empty() {
        out.push_str("## Locations\n\n");
        for location in &board.locations {
            let _ = writeln!(out, "### {}\n", location.name.trim());
            let notes = location.notes.trim();
            if !notes.is_empty() {
                let _ = writeln!(out, "{notes}\n");
            }
        }
    }

    finish(out)
}

/// The prose of every non-archived scene in board order, one heading per
/// scene, ready to paste into a word processor.
pub fn manuscript_markdown(board: &Board) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# {}\n", novel_title(board));
    for scene in board.scenes.iter().filter(|s| s.status != Status::Archived) {
        let scene_title = scene.title.trim();
        let _ = writeln!(
            out,
            "## {}\n",
            if scene_title.is_empty() {
                "* * *"
            } else {
                scene_title
            }
        );
        let text = scene.text.trim();
        if !text.is_empty() {
            let _ = writeln!(out, "{text}\n");
        }
    }
    finish(out)
}

/// The outline and the manuscript under one title, each a section with its
/// own headings nested one level deeper.
pub fn combined_markdown(board: &Board) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# {}\n", novel_title(board));
    for (section, document) in [
        ("Outline", to_markdown(board)),
        ("Manuscript", manuscript_markdown(board)),
    ] {
        let _ = writeln!(out, "## {section}\n");
        let body = without_title(&document);
        if body.is_empty() {
            continue;
        }
        for line in body.lines() {
            if line.starts_with('#') {
                out.push('#');
            }
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
    }
    finish(out)
}

fn without_title(document: &str) -> &str {
    document
        .split_once('\n')
        .map_or("", |(_, rest)| rest.trim_start_matches('\n'))
}

fn novel_title(board: &Board) -> &str {
    let title = board.title.trim();
    if title.is_empty() {
        "Untitled Novel"
    } else {
        title
    }
}

fn finish(mut out: String) -> String {
    let trimmed_len = out.trim_end().len();
    out.truncate(trimmed_len);
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Act;

    #[test]
    fn renders_scenes_in_board_order_with_details() {
        let mut board = Board {
            title: "The Long Night".into(),
            ..Board::default()
        };
        board.add_character("Ada");
        board.characters[0].notes = "Wants revenge".into();
        let ada = board.characters[0].id.clone();

        let rain = board.add_scene();
        let scene = board.scene_mut(&rain).unwrap();
        scene.title = "Rain".into();
        scene.summary = "Ada finds the letter.".into();
        scene.status = Status::Draft;
        scene.act = Some(Act::One);
        scene.pov = Some(ada);

        let cut = board.add_scene();
        board.scene_mut(&cut).unwrap().status = Status::Archived;
        board.add_scene();

        assert_eq!(
            to_markdown(&board),
            "# The Long Night\n\n\
             ## Scene 01: Rain\n\n\
             _Draft · Act I · POV: Ada_\n\n\
             Ada finds the letter.\n\n\
             ## Scene 03: Untitled\n\n\
             _Outline_\n\n\
             ## Characters\n\n\
             ### Ada\n\n\
             Wants revenge\n"
        );
    }

    #[test]
    fn outline_lists_worldbuilding_details() {
        let mut board = Board::default();
        board.add_character("Ada");
        board.add_character("Bo");
        board.characters[0].role = "Detective".into();
        let [ada, bo] = [0, 1].map(|i| board.characters[i].id.clone());
        board.add_relationship(&ada, &bo, "mentor");
        board.add_location("Harbor");
        board.locations[0].notes = "Fog every morning".into();
        let harbor = board.locations[0].id.clone();
        let scene = board.add_scene();
        let scene = board.scene_mut(&scene).unwrap();
        scene.location = Some(harbor);
        scene.day = Some(2);

        let outline = to_markdown(&board);

        assert!(outline.contains("_Outline · Location: Harbor · Day 2_"));
        assert!(outline.contains("### Ada\n\n**Role:** Detective\n\n- mentor → Bo\n\n### Bo"));
        assert!(outline.ends_with("## Locations\n\n### Harbor\n\nFog every morning\n"));
    }

    #[test]
    fn manuscript_holds_prose_of_kept_scenes() {
        let mut board = Board {
            title: "Dune".into(),
            ..Board::default()
        };
        let opening = board.add_scene();
        let scene = board.scene_mut(&opening).unwrap();
        scene.title = "Arrakis".into();
        scene.text = "  The spice must flow.  ".into();
        let cut = board.add_scene();
        let scene = board.scene_mut(&cut).unwrap();
        scene.text = "Cut.".into();
        scene.status = Status::Archived;
        board.add_scene();

        assert_eq!(
            manuscript_markdown(&board),
            "# Dune\n\n## Arrakis\n\nThe spice must flow.\n\n## * * *\n"
        );
    }

    #[test]
    fn combined_nests_outline_and_manuscript_under_one_title() {
        let mut board = Board {
            title: "Dune".into(),
            ..Board::default()
        };
        board.add_character("Paul");
        let opening = board.add_scene();
        let scene = board.scene_mut(&opening).unwrap();
        scene.title = "Arrakis".into();
        scene.summary = "Paul arrives.".into();
        scene.text = "The spice must flow.".into();

        assert_eq!(
            combined_markdown(&board),
            "# Dune\n\n\
             ## Outline\n\n\
             ### Scene 01: Arrakis\n\n\
             _Outline_\n\n\
             Paul arrives.\n\n\
             ### Characters\n\n\
             #### Paul\n\n\
             ## Manuscript\n\n\
             ### Arrakis\n\n\
             The spice must flow.\n"
        );
    }

    #[test]
    fn combined_empty_board_keeps_both_sections() {
        assert_eq!(
            combined_markdown(&Board::default()),
            "# Untitled Novel\n\n## Outline\n\n## Manuscript\n"
        );
    }

    #[test]
    fn empty_board_is_just_a_heading() {
        assert_eq!(to_markdown(&Board::default()), "# Untitled Novel\n");
    }
}
