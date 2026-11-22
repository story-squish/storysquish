use std::fmt::Write;

use crate::model::{Board, Status};

/// Archived scenes are cut material, so they stay out of the outline; the
/// remaining scenes keep their board numbers so they match the cards.
pub fn to_markdown(board: &Board) -> String {
    let mut out = String::new();
    let title = board.title.trim();
    let _ = writeln!(
        out,
        "# {}\n",
        if title.is_empty() {
            "Untitled Novel"
        } else {
            title
        }
    );

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
            let notes = character.notes.trim();
            if !notes.is_empty() {
                let _ = writeln!(out, "{notes}\n");
            }
        }
    }

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
    fn empty_board_is_just_a_heading() {
        assert_eq!(to_markdown(&Board::default()), "# Untitled Novel\n");
    }
}
