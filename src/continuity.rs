//! Cross-scene checks: facts recorded once on the board are compared with
//! every scene, in reading order, and disagreements are reported.

use crate::model::{Board, Id, Scene, Status};

/// A scene that disagrees with something recorded elsewhere on the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Issue {
    AppearsBeforeArrival {
        character: Id,
        scene: usize,
        arrival: usize,
        count: usize,
    },
    AppearsAfterDeath {
        character: Id,
        scene: usize,
        death: usize,
        count: usize,
    },
    ItemAppearsAfterResolution {
        item: Id,
        scene: usize,
        resolved: usize,
    },
    ItemDropsOut {
        item: Id,
        last: usize,
        remaining: usize,
    },
    LocationJump {
        from: usize,
        to: usize,
    },
    TimeRunsBackwards {
        from: usize,
        to: usize,
    },
}

impl Issue {
    /// The scene to open when the issue is picked: the one that breaks the
    /// recorded fact, or the last sighting for an item that drops out.
    pub fn scene(&self) -> usize {
        match self {
            Self::AppearsBeforeArrival { scene, .. }
            | Self::AppearsAfterDeath { scene, .. }
            | Self::ItemAppearsAfterResolution { scene, .. } => *scene,
            Self::ItemDropsOut { last, .. } => *last,
            Self::LocationJump { to, .. } | Self::TimeRunsBackwards { to, .. } => *to,
        }
    }

    pub fn message(&self, board: &Board) -> String {
        match self {
            Self::AppearsBeforeArrival {
                character, count, ..
            } => {
                let name = character_name(board, character);
                if *count == 1 {
                    format!("{name} appears before arriving")
                } else {
                    format!("{name} appears in {count} scenes before arriving")
                }
            }
            Self::AppearsAfterDeath {
                character, count, ..
            } => {
                let name = character_name(board, character);
                if *count == 1 {
                    format!("{name} appears after dying")
                } else {
                    format!("{name} appears in {count} scenes after dying")
                }
            }
            Self::ItemAppearsAfterResolution { item, .. } => {
                format!("{} appears after being resolved", item_name(board, item))
            }
            Self::ItemDropsOut {
                item, remaining, ..
            } => format!(
                "{} is last seen {remaining} scene{} before the end",
                item_name(board, item),
                if *remaining == 1 { "" } else { "s" }
            ),
            Self::LocationJump { from, to } => format!(
                "{} → {} on the same day, in different regions",
                location_name(board, *from),
                location_name(board, *to)
            ),
            Self::TimeRunsBackwards { from, to } => {
                let time = |index: usize| {
                    board
                        .scenes
                        .get(index)
                        .and_then(|s| s.time)
                        .map_or_else(|| "?".to_owned(), |t| t.to_string())
                };
                format!("{} → {} on the same day", time(*from), time(*to))
            }
        }
    }
}

/// Every disagreement between recorded facts and the manuscript, in reading
/// order. Flashbacks are left out: they are told out of order on purpose.
pub fn issues(board: &Board) -> Vec<Issue> {
    let mut issues = Vec::new();
    let manuscript: Vec<usize> = board
        .scenes
        .iter()
        .enumerate()
        .filter(|(_, scene)| scene.status != Status::Archived)
        .map(|(index, _)| index)
        .collect();

    for (character, appearances) in board.character_appearances() {
        let boundary = |scene: &Option<Id>| scene.as_ref().and_then(|id| board.scene_index(id));
        if let Some(arrival) = boundary(&character.arrival) {
            let early: Vec<usize> = appearances
                .iter()
                .copied()
                .filter(|&index| index < arrival && !is_flashback(board, index))
                .collect();
            if !is_flashback(board, arrival)
                && let Some(&scene) = early.first()
            {
                issues.push(Issue::AppearsBeforeArrival {
                    character: character.id.clone(),
                    scene,
                    arrival,
                    count: early.len(),
                });
            }
        }
        if let Some(death) = boundary(&character.death) {
            let late: Vec<usize> = appearances
                .iter()
                .copied()
                .filter(|&index| index > death && !is_flashback(board, index))
                .collect();
            if !is_flashback(board, death)
                && let Some(&scene) = late.first()
            {
                issues.push(Issue::AppearsAfterDeath {
                    character: character.id.clone(),
                    scene,
                    death,
                    count: late.len(),
                });
            }
        }
    }

    issues.extend(item_issues(board, &manuscript));
    issues.extend(timeline_issues(board, &manuscript));
    issues.sort_by_key(Issue::scene);
    issues
}

fn item_issues(board: &Board, manuscript: &[usize]) -> Vec<Issue> {
    let mut issues = Vec::new();
    for item in &board.items {
        let mentions: Vec<usize> = manuscript
            .iter()
            .copied()
            .filter(|&index| mentions(&board.scenes[index], &item.name))
            .collect();
        let Some(&last) = mentions.last() else {
            continue;
        };
        if let Some(resolved) = item.resolved.as_ref().and_then(|id| board.scene_index(id))
            && let Some(&scene) = mentions.iter().find(|&&index| index > resolved)
            && !is_flashback(board, scene)
            && !is_flashback(board, resolved)
        {
            issues.push(Issue::ItemAppearsAfterResolution {
                item: item.id.clone(),
                scene,
                resolved,
            });
        }
        let remaining = manuscript.iter().filter(|&&index| index > last).count();
        if item.resolved.is_none() && remaining >= 2 {
            issues.push(Issue::ItemDropsOut {
                item: item.id.clone(),
                last,
                remaining,
            });
        }
    }
    issues
}

fn timeline_issues(board: &Board, manuscript: &[usize]) -> Vec<Issue> {
    let days = board.story_days();
    let mut issues = Vec::new();
    for pair in manuscript.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        if is_flashback(board, from) || is_flashback(board, to) {
            continue;
        }
        let Some(day) = days[from] else {
            continue;
        };
        if days[to] != Some(day) {
            continue;
        }
        if let (Some(before), Some(after)) = (board.scenes[from].time, board.scenes[to].time)
            && after < before
        {
            issues.push(Issue::TimeRunsBackwards { from, to });
        } else if location_jumps(board, &board.scenes[from], &board.scenes[to]) {
            issues.push(Issue::LocationJump { from, to });
        }
    }
    issues
}

fn location_jumps(board: &Board, from: &Scene, to: &Scene) -> bool {
    let (Some(left), Some(right)) = (
        from.location.as_ref().and_then(|id| board.location(id)),
        to.location.as_ref().and_then(|id| board.location(id)),
    ) else {
        return false;
    };
    let (left_region, right_region) = (left.region.trim(), right.region.trim());
    left.id != right.id
        && !left_region.is_empty()
        && !right_region.is_empty()
        && !left_region.eq_ignore_ascii_case(right_region)
}

fn is_flashback(board: &Board, index: usize) -> bool {
    board.scenes.get(index).is_some_and(|scene| scene.flashback)
}

/// Whole-word, case-insensitive match, so "the locket" is found in prose but
/// "locket" is not matched inside another word.
fn mentions(scene: &Scene, name: &str) -> bool {
    let needle = name.trim().to_lowercase();
    !needle.is_empty()
        && [&scene.title, &scene.summary, &scene.text]
            .into_iter()
            .any(|field| contains_word(&field.to_lowercase(), &needle))
}

fn contains_word(text: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(offset) = text[from..].find(needle) {
        let start = from + offset;
        let end = start + needle.len();
        let before_ok = text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let after_ok = text[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric());
        if before_ok && after_ok {
            return true;
        }
        from = start + needle.len().max(1);
    }
    false
}

fn character_name(board: &Board, id: &Id) -> String {
    board
        .character(id)
        .map_or_else(|| "A character".to_owned(), |c| display(&c.name))
}

fn item_name(board: &Board, id: &Id) -> String {
    board
        .item(id)
        .map_or_else(|| "An item".to_owned(), |i| display(&i.name))
}

fn location_name(board: &Board, index: usize) -> String {
    board
        .scenes
        .get(index)
        .and_then(|scene| scene.location.as_ref())
        .and_then(|id| board.location(id))
        .map_or_else(|| "Somewhere".to_owned(), |l| display(&l.name))
}

fn display(name: &str) -> String {
    if name.trim().is_empty() {
        "Unnamed".to_owned()
    } else {
        name.trim().to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TimeOfDay;

    fn scene(board: &mut Board, text: &str) -> Id {
        let id = board.add_scene();
        board.scene_mut(&id).unwrap().text = text.to_owned();
        id
    }

    fn day(board: &mut Board, id: &Id, number: i32) {
        board.scene_mut(id).unwrap().day = Some(number);
    }

    fn time(board: &mut Board, id: &Id, time: TimeOfDay) {
        board.scene_mut(id).unwrap().time = Some(time);
    }

    fn located(board: &mut Board, id: &Id, location: &Id) {
        board.scene_mut(id).unwrap().location = Some(location.clone());
    }

    fn place(board: &mut Board, name: &str, region: &str) -> Id {
        board.add_location(name);
        let location = board.locations.last_mut().unwrap();
        location.region = region.to_owned();
        location.id.clone()
    }

    #[test]
    fn a_character_named_before_arriving_is_flagged_once() {
        let mut board = Board::default();
        board.add_character("Ada");
        let ada = board.characters[0].id.clone();
        scene(&mut board, "Ada waits.");
        scene(&mut board, "Ada leaves.");
        let arrival = scene(&mut board, "Ada arrives.");
        board.character_mut(&ada).unwrap().arrival = Some(arrival);

        let issues = issues(&board);
        assert_eq!(
            issues,
            vec![Issue::AppearsBeforeArrival {
                character: ada,
                scene: 0,
                arrival: 2,
                count: 2,
            }]
        );
        assert_eq!(
            issues[0].message(&board),
            "Ada appears in 2 scenes before arriving"
        );
    }

    #[test]
    fn a_character_seen_after_dying_is_flagged() {
        let mut board = Board::default();
        board.add_character("Ada");
        let ada = board.characters[0].id.clone();
        let death = scene(&mut board, "Ada dies.");
        scene(&mut board, "The funeral.");
        scene(&mut board, "Ada smiles.");
        board.character_mut(&ada).unwrap().death = Some(death);

        assert_eq!(
            issues(&board),
            vec![Issue::AppearsAfterDeath {
                character: ada,
                scene: 2,
                death: 0,
                count: 1,
            }]
        );
    }

    #[test]
    fn flashbacks_are_left_alone() {
        let mut board = Board::default();
        board.add_character("Ada");
        let ada = board.characters[0].id.clone();
        let flashback = scene(&mut board, "Young Ada plays.");
        board.scene_mut(&flashback).unwrap().flashback = true;
        let arrival = scene(&mut board, "Ada arrives.");
        board.character_mut(&ada).unwrap().arrival = Some(arrival);
        board.character_mut(&ada).unwrap().death = Some(flashback);
        scene(&mut board, "Ada lives.");

        assert_eq!(issues(&board), []);
    }

    #[test]
    fn an_item_that_drops_out_is_reported() {
        let mut board = Board::default();
        board.add_item("the locket");
        let item = board.items[0].id.clone();
        scene(&mut board, "She finds the locket.");
        scene(&mut board, "");
        scene(&mut board, "The end.");

        assert_eq!(
            issues(&board),
            vec![Issue::ItemDropsOut {
                item: item.clone(),
                last: 0,
                remaining: 2,
            }]
        );
        assert_eq!(
            issues(&board)[0].message(&board),
            "the locket is last seen 2 scenes before the end"
        );
    }

    #[test]
    fn a_resolved_item_is_only_flagged_if_it_comes_back() {
        let mut board = Board::default();
        board.add_item("the locket");
        let item = board.items[0].id.clone();
        let found = scene(&mut board, "She finds the locket.");
        let burned = scene(&mut board, "The locket burns.");
        board.item_mut(&item).unwrap().resolved = Some(burned.clone());
        assert_eq!(issues(&board), []);

        scene(&mut board, "The locket glints.");
        assert_eq!(
            issues(&board),
            vec![Issue::ItemAppearsAfterResolution {
                item,
                scene: 2,
                resolved: 1,
            }]
        );
        assert!(board.scene_index(&found).is_some());
    }

    #[test]
    fn item_names_match_whole_words_only() {
        let mut board = Board::default();
        board.add_item("ark");
        scene(&mut board, "The dark bark.");
        scene(&mut board, "");
        scene(&mut board, "");
        assert_eq!(issues(&board), [], "ark is not inside dark or bark");

        board.scenes[0].text = "The Ark sails.".to_owned();
        assert_eq!(issues(&board).len(), 1);
    }

    #[test]
    fn a_same_day_jump_between_regions_is_flagged() {
        let mut board = Board::default();
        let harbor = place(&mut board, "Harbor", "Coast");
        let castle = place(&mut board, "Castle", "Highlands");
        let first = scene(&mut board, "");
        let second = scene(&mut board, "");
        day(&mut board, &first, 1);
        day(&mut board, &second, 1);
        located(&mut board, &first, &harbor);
        located(&mut board, &second, &castle);

        assert_eq!(issues(&board), vec![Issue::LocationJump { from: 0, to: 1 }]);
        assert_eq!(
            issues(&board)[0].message(&board),
            "Harbor → Castle on the same day, in different regions"
        );
    }

    #[test]
    fn a_jump_within_one_region_or_across_days_is_fine() {
        let mut board = Board::default();
        let harbor = place(&mut board, "Harbor", "Coast");
        let castle = place(&mut board, "Castle", "Coast");
        let first = scene(&mut board, "");
        let second = scene(&mut board, "");
        day(&mut board, &first, 1);
        day(&mut board, &second, 1);
        located(&mut board, &first, &harbor);
        located(&mut board, &second, &castle);
        assert_eq!(issues(&board), []);

        day(&mut board, &second, 2);
        assert_eq!(issues(&board), [], "a day of travel is enough");
    }

    #[test]
    fn a_clock_that_runs_backwards_on_one_day_is_flagged() {
        let mut board = Board::default();
        let first = scene(&mut board, "");
        let second = scene(&mut board, "");
        day(&mut board, &first, 3);
        day(&mut board, &second, 3);
        time(&mut board, &first, TimeOfDay::Evening);
        time(&mut board, &second, TimeOfDay::Morning);

        assert_eq!(
            issues(&board),
            vec![Issue::TimeRunsBackwards { from: 0, to: 1 }]
        );
        assert_eq!(
            issues(&board)[0].message(&board),
            "Evening → Morning on the same day"
        );

        time(&mut board, &second, TimeOfDay::Night);
        assert_eq!(issues(&board), []);
    }

    #[test]
    fn issues_follow_reading_order_and_skip_archived_scenes() {
        let mut board = Board::default();
        board.add_item("the locket");
        let item = board.items[0].id.clone();
        scene(&mut board, "the locket");
        let hidden = scene(&mut board, "the locket");
        board.scene_mut(&hidden).unwrap().status = Status::Archived;
        scene(&mut board, "");
        scene(&mut board, "");
        board.item_mut(&item).unwrap().resolved = None;

        assert_eq!(issues(&board)[0].scene(), 0);
    }

    #[test]
    fn locations_without_regions_are_not_compared() {
        let mut board = Board::default();
        let harbor = place(&mut board, "Harbor", "");
        let castle = place(&mut board, "Castle", "Highlands");
        let first = scene(&mut board, "");
        let second = scene(&mut board, "");
        day(&mut board, &first, 1);
        day(&mut board, &second, 1);
        located(&mut board, &first, &harbor);
        located(&mut board, &second, &castle);
        assert_eq!(issues(&board), []);
        assert_eq!(board.location(&castle).unwrap().region, "Highlands");
    }
}
