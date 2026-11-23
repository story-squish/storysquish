use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use storysquish::app::PlannerApp;
use storysquish::model::Board;
use storysquish::theme::ColorScheme;
use tempfile::TempDir;

const WINDOW: egui::Vec2 = egui::vec2(1200.0, 800.0);

struct Fixture {
    harness: Harness<'static, PlannerApp>,
    dir: TempDir,
}

fn launch(board: Board) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.json");
    let settings_path = dir.path().join("config/storysquish/settings.json");
    let mut harness = Harness::builder()
        .with_size(WINDOW)
        .build_eframe(move |cc| PlannerApp::new(cc, board, path, settings_path, None));
    harness.run();
    Fixture { harness, dir }
}

fn board_with_scenes(titles: &[&str]) -> Board {
    let mut board = Board::default();
    for title in titles {
        let id = board.add_scene();
        board.scene_mut(&id).unwrap().title = (*title).to_owned();
    }
    board
}

fn labelled_text_input<'h>(
    harness: &'h Harness<'_, PlannerApp>,
    label: &'h str,
) -> egui_kittest::Node<'h> {
    harness.get_by(|node| {
        matches!(
            node.role(),
            egui::accesskit::Role::MultilineTextInput
                | egui::accesskit::Role::TextInput
                | egui::accesskit::Role::PasswordInput
        ) && node.label().as_deref() == Some(label)
    })
}

fn scene_titles(harness: &Harness<'_, PlannerApp>) -> Vec<String> {
    harness
        .state()
        .board()
        .scenes
        .iter()
        .map(|s| s.title.clone())
        .collect()
}

#[test]
fn new_scene_button_adds_card_and_opens_inspector() {
    let Fixture { mut harness, .. } = launch(Board::default());

    harness.get_by_label("+ Scene").click();
    harness.run();

    assert_eq!(harness.state().board().scenes.len(), 1);
    assert!(harness.query_by_label("Delete scene").is_some());
}

fn many_scene_titles(count: usize) -> Vec<String> {
    (1..=count).map(|n| format!("Scene title {n}")).collect()
}

#[test]
fn stats_panel_is_on_screen_below_board() {
    let titles = many_scene_titles(40);
    let titles: Vec<&str> = titles.iter().map(String::as_str).collect();
    let Fixture { mut harness, .. } = launch(board_with_scenes(&titles));

    harness.get_by_label("Stats").click();
    harness.run();

    let total = harness.get_by_label("Total").rect();
    let first_card = harness.get_by_label("Scene title 1").rect();
    assert!(
        total.bottom() <= WINDOW.y,
        "statistics panel is pushed off screen: {total:?}"
    );
    assert!(first_card.bottom() < harness.get_by_label("Statistics").rect().top());
}

#[test]
fn cards_wrap_within_window_width() {
    let titles = many_scene_titles(12);
    let titles: Vec<&str> = titles.iter().map(String::as_str).collect();
    let Fixture { harness, .. } = launch(board_with_scenes(&titles));

    let cards: Vec<_> = harness
        .get_all_by_label_contains("Scene title")
        .map(|node| node.rect())
        .collect();
    assert_eq!(cards.len(), 12);
    for card in &cards {
        assert!(card.right() <= WINDOW.x, "card runs off screen: {card:?}");
    }
    let rows = cards
        .iter()
        .map(|r| r.top().to_bits())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(rows.len() > 1, "all cards were laid out on one row");
}

#[test]
fn dragging_card_onto_another_reorders_scenes() {
    let Fixture { mut harness, .. } = launch(board_with_scenes(&["A", "B", "C"]));

    let from = harness.get_by_label("C").rect().center();
    let to = harness.get_by_label("A").rect().center();

    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    for step in 1..=10 {
        #[expect(clippy::cast_precision_loss, reason = "tiny step count")]
        let t = step as f32 / 10.0;
        harness.hover_at(from.lerp(to, t));
        harness.run();
    }
    harness.drop_at(to);
    harness.run();

    assert_eq!(scene_titles(&harness), ["C", "A", "B"]);
}

#[test]
fn deleted_scene_comes_back_with_undo() {
    let Fixture { mut harness, .. } = launch(board_with_scenes(&["Keep me"]));

    harness.get_by_label("Keep me").click();
    harness.run();
    harness.get_by_label("Delete scene").click();
    harness.run();
    harness.get_by_label("Delete").click();
    harness.run();
    assert_eq!(harness.state().board().scenes.len(), 0);

    harness.get_by_label("Undo").click();
    harness.run();
    assert_eq!(scene_titles(&harness), ["Keep me"]);
}

#[test]
fn escape_closes_inspector() {
    let Fixture { mut harness, .. } = launch(board_with_scenes(&["Rain"]));

    harness.get_by_label("Rain").click();
    harness.run();
    assert!(harness.query_by_label("Delete scene").is_some());

    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(harness.query_by_label("Delete scene").is_none());
}

#[test]
fn character_notes_are_editable_from_inspector() {
    let mut board = Board::default();
    board.add_character("Ada");
    let Fixture { mut harness, .. } = launch(board);

    harness.get_by_label("Ada").click();
    harness.run();
    assert!(harness.query_by_label("Delete character").is_some());
    labelled_text_input(&harness, "NOTES").focus();
    harness.run();
    labelled_text_input(&harness, "NOTES").type_text("Wants revenge");
    harness.run();

    assert_eq!(harness.state().board().characters[0].notes, "Wants revenge");
}

#[test]
fn character_gender_is_editable_from_inspector() {
    let mut board = Board::default();
    board.add_character("Ada");
    let Fixture { mut harness, .. } = launch(board);

    harness.get_by_label("Ada").click();
    harness.run();
    labelled_text_input(&harness, "GENDER").focus();
    harness.run();
    labelled_text_input(&harness, "GENDER").type_text("Woman");
    harness.run();

    assert_eq!(harness.state().board().characters[0].gender, "Woman");
}

fn type_character_notes(harness: &mut Harness<'_, PlannerApp>, notes: &str) {
    harness.get_by_label("Ada").click();
    harness.run();
    labelled_text_input(harness, "NOTES").focus();
    harness.run();
    labelled_text_input(harness, "NOTES").type_text(notes);
    harness.run();
}

fn saved_character_notes(dir: &TempDir) -> String {
    storysquish::storage::load(&dir.path().join("board.json"))
        .map_or_default(|board| board.characters[0].notes.clone())
}

#[test]
fn leaving_a_text_field_saves_its_edits() {
    let mut board = Board::default();
    board.add_character("Ada");
    let Fixture { mut harness, dir } = launch(board);

    type_character_notes(&mut harness, "Wants revenge");
    labelled_text_input(&harness, "GENDER").focus();
    harness.run();

    assert_eq!(saved_character_notes(&dir), "Wants revenge");
}

#[test]
fn switching_away_from_the_window_saves_pending_edits() {
    let mut board = Board::default();
    board.add_character("Ada");
    let Fixture { mut harness, dir } = launch(board);

    type_character_notes(&mut harness, "Wants revenge");
    harness.input_mut().focused = false;
    harness.event(egui::Event::WindowFocused(false));
    harness.run();

    assert_eq!(saved_character_notes(&dir), "Wants revenge");
}

#[test]
fn editing_the_title_saves_without_waiting() {
    let board = Board {
        title: "Dune".to_owned(),
        ..Board::default()
    };
    let Fixture { mut harness, dir } = launch(board);

    harness.get_all_by_value("Dune").next().unwrap().focus();
    harness.run();
    harness.event(egui::Event::Text(" Messiah".to_owned()));
    harness.run();

    let saved = storysquish::storage::load(&dir.path().join("Dune_Messiah.json")).unwrap();
    assert_eq!(saved.title, "Dune Messiah");
}

#[test]
fn a_board_file_named_after_its_title_follows_a_new_title() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Untitled_Novel_2.json");
    let board = Board {
        title: "Dune".to_owned(),
        ..Board::default()
    };
    storysquish::storage::save(&path, &board).unwrap();
    storysquish::storage::back_up(&path, "", 5).unwrap();
    let settings_path = dir.path().join("settings.json");
    let mut harness = Harness::builder()
        .with_size(WINDOW)
        .build_eframe(move |cc| PlannerApp::new(cc, board, path, settings_path, None));
    harness.run();
    assert_eq!(harness.state().board_path(), dir.path().join("Dune.json"));

    harness.get_all_by_value("Dune").next().unwrap().focus();
    harness.run();
    harness.event(egui::Event::Text(" Messiah".to_owned()));
    harness.run();

    let renamed = dir.path().join("Dune_Messiah.json");
    assert_eq!(harness.state().board_path(), renamed);
    assert_eq!(
        storysquish::storage::load(&renamed).unwrap().title,
        "Dune Messiah"
    );
    assert!(!dir.path().join("Dune.json").exists());
    assert!(!dir.path().join("Untitled_Novel_2-backups").exists());
    let backups = storysquish::storage::list_backups(&renamed).unwrap();
    assert!(!backups.is_empty());
    assert!(backups.iter().all(|backup| backup.saved_at.is_some()));
    let settings = storysquish::settings::load(&dir.path().join("settings.json")).unwrap();
    assert_eq!(settings.board_path.as_deref(), Some(renamed.as_path()));
}

#[test]
fn a_board_file_named_by_hand_keeps_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("draft.json");
    let board = Board {
        title: "Dune".to_owned(),
        ..Board::default()
    };
    storysquish::storage::save(&path, &board).unwrap();
    let settings_path = dir.path().join("settings.json");
    let mut harness = Harness::builder()
        .with_size(WINDOW)
        .build_eframe(move |cc| PlannerApp::new(cc, board, path, settings_path, None));
    harness.run();

    harness.get_all_by_value("Dune").next().unwrap().focus();
    harness.run();
    harness.event(egui::Event::Text(" Messiah".to_owned()));
    harness.run();

    assert_eq!(harness.state().board_path(), dir.path().join("draft.json"));
}

#[test]
fn searching_does_not_mark_board_unsaved() {
    let Fixture { mut harness, .. } = launch(board_with_scenes(&["Rain", "Sun"]));

    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
    harness.run();
    harness.event(egui::Event::Text("Rain".to_owned()));
    harness.run();

    assert!(harness.query_by_label("saved").is_some());
    assert!(harness.query_by_label("saving…").is_none());
    assert!(harness.query_by_label("Sun").is_none());
}

#[test]
fn scenes_with_a_pov_are_shown_without_a_pov_filter() {
    let mut board = board_with_scenes(&["Rain", "Sun"]);
    board.add_character("Ada");
    board.scenes[0].pov = Some(board.characters[0].id.clone());
    let Fixture { harness, .. } = launch(board);

    assert!(harness.query_by_label("Rain").is_some());
    assert!(harness.query_by_label("Sun").is_some());
}

#[test]
fn duplicate_button_copies_selected_scene() {
    let Fixture { mut harness, .. } = launch(board_with_scenes(&["Rain", "Sun"]));

    harness.get_by_label("Rain").click();
    harness.run();
    harness.get_by_label("Duplicate").click();
    harness.run();

    assert_eq!(scene_titles(&harness), ["Rain", "Rain (copy)", "Sun"]);
}

#[test]
fn selected_scene_moves_with_buttons_and_keyboard() {
    let Fixture { mut harness, .. } = launch(board_with_scenes(&["A", "B", "C"]));

    harness.get_by_label("A").click();
    harness.run();
    harness.get_by_label("Move later").click();
    harness.run();
    assert_eq!(scene_titles(&harness), ["B", "A", "C"]);

    harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowRight);
    harness.run();
    assert_eq!(scene_titles(&harness), ["B", "C", "A"]);

    harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowRight);
    harness.run();
    assert_eq!(scene_titles(&harness), ["B", "C", "A"]);

    harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowLeft);
    harness.run();
    assert_eq!(scene_titles(&harness), ["B", "A", "C"]);
}

#[test]
fn clear_filters_restores_hidden_scenes() {
    let Fixture { mut harness, .. } = launch(board_with_scenes(&["Rain", "Sun"]));
    assert!(harness.query_by_label("Clear filters").is_none());

    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
    harness.run();
    harness.event(egui::Event::Text("Rain".to_owned()));
    harness.run();
    assert!(harness.query_by_label("Sun").is_none());

    harness.get_by_label("Clear filters").click();
    harness.run();
    assert!(harness.query_by_label("Sun").is_some());
    assert!(harness.query_by_label("Clear filters").is_none());
}

#[test]
fn window_title_follows_novel_title() {
    let board = Board {
        title: "Dune".to_owned(),
        ..Board::default()
    };
    let Fixture { mut harness, .. } = launch(board);
    assert_eq!(harness.state().window_title(), "Dune — StorySquish");

    harness.get_all_by_value("Dune").next().unwrap().focus();
    harness.run();
    harness.event(egui::Event::Text(" Messiah".to_owned()));
    harness.run();
    assert_eq!(harness.state().window_title(), "Dune Messiah — StorySquish");
}

#[test]
fn selected_character_moves_with_buttons_and_keyboard() {
    let mut board = Board::default();
    for name in ["Ada", "Bo", "Cy"] {
        board.add_character(name);
    }
    let Fixture { mut harness, .. } = launch(board);
    let names = |harness: &Harness<'_, PlannerApp>| -> Vec<String> {
        harness
            .state()
            .board()
            .characters
            .iter()
            .map(|c| c.name.clone())
            .collect()
    };

    harness.get_by_label("Ada").click();
    harness.run();
    harness.get_by_label("Move down").click();
    harness.run();
    assert_eq!(names(&harness), ["Bo", "Ada", "Cy"]);

    harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowUp);
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowUp);
    harness.run();
    assert_eq!(names(&harness), ["Ada", "Bo", "Cy"]);
}

#[test]
fn choosing_a_color_scheme_applies_and_saves_it() {
    let Fixture { mut harness, dir } = launch(board_with_scenes(&["Rain"]));
    assert_eq!(harness.state().color_scheme(), ColorScheme::Parchment);

    harness.get_by_label("Settings").click();
    harness.run();
    harness
        .get_by(|node| {
            node.role() == egui::accesskit::Role::ComboBox
                && node.label().as_deref() == Some("COLOR SCHEME")
        })
        .click();
    harness.run();
    harness.get_by_label("Midnight").click();
    harness.run();

    assert_eq!(harness.state().color_scheme(), ColorScheme::Midnight);
    assert_eq!(harness.ctx.theme(), egui::Theme::Dark);
    let saved =
        storysquish::settings::load(&dir.path().join("config/storysquish/settings.json")).unwrap();
    assert_eq!(saved.color_scheme, ColorScheme::Midnight);

    harness.get_by_label("Close").click();
    harness.run();
    assert!(harness.query_by_label("Close").is_none());
}

#[test]
fn saved_color_scheme_is_restored_on_launch() {
    let dir = tempfile::tempdir().unwrap();
    let settings_path = dir.path().join("settings.json");
    std::fs::write(&settings_path, r#"{"color-scheme": "ember"}"#).unwrap();
    let board_path = dir.path().join("board.json");
    let harness = Harness::builder()
        .with_size(WINDOW)
        .build_eframe(move |cc| {
            PlannerApp::new(cc, Board::default(), board_path, settings_path, None)
        });

    assert_eq!(harness.state().color_scheme(), ColorScheme::Ember);
}

#[test]
fn scene_card_stacks_number_title_and_badges() {
    let mut board = board_with_scenes(&["Rain"]);
    board.scenes[0].act = Some(storysquish::model::Act::Two);
    let Fixture { harness, .. } = launch(board);

    let number = harness.get_by_label("SCENE 01").rect();
    let title = harness.get_by_label("Rain").rect();
    let act = harness.get_by_label("ACT II").rect();
    assert!(title.top() >= number.bottom(), "{number:?} {title:?}");
    assert!(act.top() >= title.bottom(), "{title:?} {act:?}");
    assert!(
        act.height() < 2.0 * title.height(),
        "badge text wrapped: {act:?}"
    );
}

#[test]
fn write_view_edits_prose_of_selected_scene() {
    let Fixture { mut harness, .. } = launch(board_with_scenes(&["Rain", "Sun"]));

    harness.get_by_label("Rain").click();
    harness.run();
    harness.get_by_label("Write prose").click();
    harness.run();
    labelled_text_input(&harness, "SCENE 01  Rain").focus();
    harness.run();
    labelled_text_input(&harness, "SCENE 01  Rain").type_text("It poured all night.");
    harness.run();
    assert_eq!(
        harness.state().board().scenes[0].text,
        "It poured all night."
    );
    assert!(
        harness
            .query_by_label("4 words · 4 in manuscript")
            .is_some()
    );

    harness.get_by_label("Next").click();
    harness.run();
    assert!(harness.query_by_label("SCENE 02  Sun").is_some());
}

#[test]
fn deleting_location_from_inspector_clears_it_from_scenes() {
    let mut board = board_with_scenes(&["Rain"]);
    board.add_location("Harbor");
    board.scenes[0].location = Some(board.locations[0].id.clone());
    board.scenes[0].day = Some(3);
    let Fixture { mut harness, .. } = launch(board);
    assert!(harness.query_by_label("Harbor · Day 3").is_some());

    harness.get_by_label("Harbor").click();
    harness.run();
    assert!(harness.query_by_label("Setting of 1 scene").is_some());
    harness.get_by_label("Delete location").click();
    harness.run();

    assert_eq!(harness.state().board().locations.len(), 0);
    assert_eq!(harness.state().board().scenes[0].location, None);
}

#[test]
fn relationship_is_added_from_character_inspector() {
    let mut board = Board::default();
    board.add_character("Ada");
    board.add_character("Bo");
    let Fixture { mut harness, .. } = launch(board);

    harness.get_by_label("Ada").click();
    harness.run();
    harness
        .get_by(|node| {
            node.role() == egui::accesskit::Role::ComboBox
                && node.label().as_deref() == Some("RELATIONSHIPS")
        })
        .click();
    harness.run();
    harness
        .get_all_by_label("Bo")
        .last()
        .expect("Bo is offered as a relationship target")
        .click();
    harness.run();
    harness.get_by_label("Add").click();
    harness.run();

    let board = harness.state().board();
    assert_eq!(board.relationships.len(), 1);
    assert_eq!(board.relationships[0].from, board.characters[0].id);
    assert_eq!(board.relationships[0].to, board.characters[1].id);
    assert!(harness.query_by_label("to Bo").is_some());
}

#[test]
fn reverse_button_adds_opposite_relationship_once() {
    let mut board = Board::default();
    board.add_character("Ada");
    board.add_character("Bo");
    let [ada, bo] = [0, 1].map(|i| board.characters[i].id.clone());
    board.add_relationship(&ada, &bo, "pimp");
    let Fixture { mut harness, .. } = launch(board);

    harness.get_by_label("Ada").click();
    harness.run();
    harness.get_by_label("Add reverse").click();
    harness.run();

    let board = harness.state().board();
    assert_eq!(board.relationships.len(), 2);
    assert_eq!(board.relationships[1].from, bo);
    assert_eq!(board.relationships[1].to, ada);
    assert!(harness.query_by_label("from Bo").is_some());
    assert!(harness.query_by_label("Add reverse").is_none());
}

#[test]
fn inspector_keeps_its_width_with_long_relationship_names() {
    let mut board = Board::default();
    board.add_character("Adelaide Montgomery-Fairweather");
    board.add_character("Bartholomew Featherstonehaugh III");
    let [ada, bo] = [0, 1].map(|i| board.characters[i].id.clone());
    board.add_relationship(&ada, &bo, "estranged cousin");
    let Fixture { mut harness, .. } = launch(board);

    harness
        .get_by_label("Adelaide Montgomery-Fairweather")
        .click();
    harness.run();
    let name = "to Bartholomew Featherstonehaugh III";
    let initial = harness.get_by_label(name).rect();
    for step in 0..10u8 {
        harness.hover_at(egui::pos2(400.0 + f32::from(step) * 10.0, 400.0));
        harness.run();
    }

    assert_eq!(harness.get_by_label(name).rect(), initial);
}

#[test]
fn timeline_lists_dated_scenes_in_story_order() {
    let mut board = board_with_scenes(&["Present", "Flashback", "Loose end"]);
    board.scenes[0].day = Some(10);
    board.scenes[1].day = Some(2);
    let Fixture { mut harness, .. } = launch(board);

    harness.get_by_label("Timeline").click();
    harness.run();

    let flashback = harness.get_by_label("Flashback").rect();
    let present = harness.get_by_label("Present").rect();
    let loose_end = harness.get_by_label("Loose end").rect();
    assert!(flashback.top() < present.top());
    assert!(present.top() < harness.get_by_label("UNDATED").rect().top());
    assert!(harness.get_by_label("UNDATED").rect().top() < loose_end.top());
}

#[test]
fn board_changed_elsewhere_is_reloaded_and_undoable() {
    let Fixture { mut harness, dir } = launch(board_with_scenes(&["Mine"]));
    let path = dir.path().join("board.json");
    storysquish::storage::save(&path, &board_with_scenes(&["Synced"])).unwrap();

    std::thread::sleep(std::time::Duration::from_millis(2100));
    harness.run();

    assert_eq!(scene_titles(&harness), ["Synced"]);
    assert!(harness.query_by_label("reloaded").is_some());
    harness.get_by_label("Undo").click();
    harness.run();
    assert_eq!(scene_titles(&harness), ["Mine"]);
}

#[test]
fn unsaved_edits_win_over_external_change_but_keep_a_copy() {
    let Fixture { mut harness, dir } = launch(board_with_scenes(&["Mine"]));
    let path = dir.path().join("board.json");
    storysquish::storage::save(&path, &board_with_scenes(&["Theirs"])).unwrap();

    harness.get_by_label("+ Scene").click();
    harness.run();

    assert!(
        harness
            .query_by_label_contains("changed outside StorySquish")
            .is_some()
    );
    let saved = storysquish::storage::load(&path).unwrap();
    assert_eq!(saved.scenes.len(), 2);
    let conflict = std::fs::read_dir(dir.path().join("board-backups"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.to_string_lossy().ends_with("-conflict.json"))
        .expect("their version is kept as a conflict backup");
    assert_eq!(
        storysquish::storage::load(&conflict).unwrap().scenes[0].title,
        "Theirs"
    );
}

#[test]
fn relationship_graph_renders_and_selects_nothing_by_default() {
    let mut board = Board::default();
    for name in ["Ada Lovelace", "Bo", "Cy"] {
        board.add_character(name);
    }
    let [ada, bo] = [0, 1].map(|i| board.characters[i].id.clone());
    board.add_relationship(&ada, &bo, "mentor");
    board.add_relationship(&bo, &ada, "rival");
    let Fixture { mut harness, .. } = launch(board);

    harness.get_by_label("Relationships").click();
    harness.run();

    assert!(harness.query_by_label("Delete character").is_none());
    assert!(
        harness
            .query_by_label_contains("add relationships in the inspector")
            .is_none()
    );
}

fn open_settings(harness: &mut Harness<'_, PlannerApp>) {
    harness.get_by_label("Settings").click();
    harness.run();
}

fn github_field<'h>(harness: &'h Harness<'_, PlannerApp>) -> egui_kittest::Node<'h> {
    labelled_text_input(harness, "GITHUB BACKUP")
}

#[test]
fn connecting_github_stores_the_token_privately_and_disconnecting_forgets_it() {
    let Fixture { mut harness, dir } = launch(board_with_scenes(&["Opening"]));
    let token_path = dir.path().join("config/storysquish/github-token");
    open_settings(&mut harness);

    github_field(&harness).focus();
    harness.run();
    github_field(&harness).type_text("ghp_example");
    harness.run();
    harness.get_by_label("Connect").click();
    harness.run();

    assert_eq!(
        storysquish::github::load_token(&token_path)
            .unwrap()
            .as_deref(),
        Some("ghp_example")
    );
    let settings = std::fs::read_to_string(dir.path().join("config/storysquish/settings.json"))
        .unwrap_or_default();
    assert!(!settings.contains("ghp_example"));
    assert!(harness.query_by_label("Back up now").is_some());
    assert!(
        harness
            .query_by_label("No repository yet; the first backup offers to create a private one.")
            .is_some()
    );

    harness.get_by_label("Disconnect").click();
    harness.run();
    assert_eq!(storysquish::github::load_token(&token_path).unwrap(), None);
    assert!(harness.query_by_label("Connect").is_some());
}

#[test]
fn set_up_github_button_opens_settings_on_the_token_until_connected() {
    let Fixture { mut harness, .. } = launch(board_with_scenes(&["Opening"]));

    harness.get_by_label("Set up GitHub backup").click();
    harness.run();
    assert!(github_field(&harness).is_focused());

    github_field(&harness).type_text("ghp_example");
    harness.run();
    harness.get_by_label("Connect").click();
    harness.run();
    assert!(harness.query_by_label("Set up GitHub backup").is_none());
}

#[test]
fn set_up_github_button_is_hidden_when_connected() {
    let (harness, _dir) = launch_connected(board_with_scenes(&["Opening"]));
    assert!(harness.query_by_label("Set up GitHub backup").is_none());
}

fn launch_connected(board: Board) -> (Harness<'static, PlannerApp>, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let settings_path = dir.path().join("settings.json");
    storysquish::github::save_token(&storysquish::github::token_path(&settings_path), "t").unwrap();
    let board_path = dir.path().join("board.json");
    let mut harness = Harness::builder()
        .with_size(WINDOW)
        .build_eframe(move |cc| PlannerApp::new(cc, board, board_path, settings_path, None));
    harness.run();
    (harness, dir)
}

#[test]
fn pasting_a_repo_url_stores_the_repo() {
    let (mut harness, dir) = launch_connected(Board::default());
    open_settings(&mut harness);

    github_field(&harness).focus();
    harness.run();
    github_field(&harness).type_text("https://github.com/someone/storysquish-dune");
    harness.run();
    assert_eq!(
        harness
            .state()
            .settings()
            .repo(harness.state().board_path()),
        Some("someone/storysquish-dune")
    );
    assert!(harness.query_by_label("Restore from GitHub…").is_some());

    harness.get_by_label("Back up automatically").click();
    harness.run();
    let saved = storysquish::settings::load(&dir.path().join("settings.json")).unwrap();
    assert_eq!(
        saved.repo(&dir.path().join("board.json")),
        Some("someone/storysquish-dune")
    );
    assert!(saved.github_auto_backup);
}

#[test]
fn new_starts_an_empty_board_in_its_own_file() {
    let (mut harness, dir) = launch_connected(board_with_scenes(&["Opening"]));
    open_settings(&mut harness);
    github_field(&harness).focus();
    harness.run();
    github_field(&harness).type_text("someone/storysquish-dune");
    harness.run();
    harness.key_press(egui::Key::Escape);
    harness.run();

    harness.get_by_label("New").click();
    harness.run();

    let old_path = dir.path().join("board.json");
    let new_path = dir.path().join("Untitled_Novel.json");
    assert!(harness.state().board().is_empty());
    assert_eq!(harness.state().board_path(), new_path);
    assert_eq!(harness.state().settings().repo(&new_path), None);
    assert_eq!(
        harness.state().settings().repo(&old_path),
        Some("someone/storysquish-dune")
    );
    assert!(storysquish::storage::load(&new_path).unwrap().is_empty());
    let saved = storysquish::settings::load(&dir.path().join("settings.json")).unwrap();
    assert_eq!(saved.board_path.as_deref(), Some(new_path.as_path()));
    assert_eq!(
        saved.previous_board_path.as_deref(),
        Some(old_path.as_path())
    );
}

#[test]
fn new_from_an_opened_backup_starts_outside_the_backups_directory() {
    let dir = tempfile::tempdir().unwrap();
    let backup = dir.path().join("board-backups/board-001795342060789.json");
    let board = board_with_scenes(&["Opening"]);
    storysquish::storage::save(&backup, &board).unwrap();
    let settings_path = dir.path().join("settings.json");
    let mut harness = Harness::builder()
        .with_size(WINDOW)
        .build_eframe(move |cc| PlannerApp::new(cc, board, backup, settings_path, None));
    harness.run();

    harness.get_by_label("New").click();
    harness.run();

    assert_eq!(
        harness.state().board_path(),
        dir.path().join("Untitled_Novel.json")
    );
}

#[test]
fn text_that_is_not_a_repo_is_flagged_and_not_saved() {
    let (mut harness, _dir) = launch_connected(Board::default());
    open_settings(&mut harness);

    github_field(&harness).focus();
    harness.run();
    github_field(&harness).type_text("my novel");
    harness.run();

    assert!(
        harness
            .query_by_label("Not a GitHub repository name or URL")
            .is_some()
    );
    assert_eq!(
        harness
            .state()
            .settings()
            .repo(harness.state().board_path()),
        None
    );
}

#[test]
fn empty_board_is_not_backed_up_to_github() {
    let (mut harness, _dir) = launch_connected(Board::default());
    open_settings(&mut harness);

    assert!(
        harness
            .get_by_label("Back up now")
            .accesskit_node()
            .is_disabled()
    );
    assert!(
        harness
            .query_by_label("Empty boards are not backed up.")
            .is_some()
    );
}

#[test]
fn open_local_switches_back_to_the_previous_board() {
    let dir = tempfile::tempdir().unwrap();
    let settings_path = dir.path().join("settings.json");
    let previous = dir.path().join("emma.json");
    storysquish::storage::save(&previous, &board_with_scenes(&["Ball"])).unwrap();
    let mut settings = storysquish::settings::Settings::default();
    settings.previous_board_path = Some(previous.clone());
    storysquish::settings::save(&settings_path, &settings).unwrap();
    let current = dir.path().join("dune.json");
    let open_path = current.clone();
    let mut harness = Harness::builder()
        .with_size(WINDOW)
        .build_eframe(move |cc| {
            PlannerApp::new(
                cc,
                board_with_scenes(&["Arrakis"]),
                open_path,
                settings_path,
                None,
            )
        });
    harness.run();

    harness.get_by_label("Open").click();
    harness.run();
    harness.get_by_label("Local").hover();
    harness.run();
    harness.get_by_label("emma.json").click();
    harness.run();

    assert_eq!(harness.state().board_path(), previous);
    assert_eq!(scene_titles(&harness), ["Ball"]);
    assert_eq!(
        harness.state().settings().previous_board_path.as_deref(),
        Some(current.as_path())
    );
}

#[test]
fn open_offers_only_local_boards_without_a_github_connection() {
    let Fixture { mut harness, .. } = launch(Board::default());
    harness.get_by_label("Open").click();
    harness.run();
    assert!(harness.query_by_label("Local").is_some());
    assert!(
        harness
            .get_by_label("Cloud…")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn open_offers_local_and_cloud_boards_with_a_github_connection() {
    let (mut harness, _dir) = launch_connected(Board::default());
    harness.get_by_label("Open").click();
    harness.run();
    assert!(harness.query_by_label("Local").is_some());
    assert!(
        !harness
            .get_by_label("Cloud…")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn open_local_offers_boards_linked_to_a_repo_with_a_github_connection() {
    let dir = tempfile::tempdir().unwrap();
    let settings_path = dir.path().join("settings.json");
    storysquish::github::save_token(&storysquish::github::token_path(&settings_path), "t").unwrap();
    let linked = dir.path().join("emma.json");
    storysquish::storage::save(&linked, &board_with_scenes(&["Ball"])).unwrap();
    let mut settings = storysquish::settings::Settings::default();
    settings.set_repo(&linked, Some("someone/storysquish-emma".to_owned()));
    storysquish::settings::save(&settings_path, &settings).unwrap();
    let open_path = dir.path().join("dune.json");
    let mut harness = Harness::builder()
        .with_size(WINDOW)
        .build_eframe(move |cc| {
            PlannerApp::new(
                cc,
                board_with_scenes(&["Arrakis"]),
                open_path,
                settings_path,
                None,
            )
            .with_github_api_base(&format!("http://{}", unused_local_address()))
        });
    harness.run();

    harness.get_by_label("Open").click();
    harness.run();
    harness.get_by_label("Local").hover();
    harness.run();
    harness.get_by_label("emma.json").click();
    harness.run();

    assert_eq!(harness.state().board_path(), linked);
    assert_eq!(scene_titles(&harness), ["Ball"]);
}

#[test]
fn export_menu_offers_outline_manuscript_and_combined_markdown() {
    let Fixture { mut harness, .. } = launch(Board::default());
    harness.get_by_label("Export").click();
    harness.run();
    for label in [
        "Board (JSON)",
        "Outline (Markdown)",
        "Manuscript (Markdown)",
        "Outline + Manuscript (Markdown)",
    ] {
        assert!(harness.query_by_label(label).is_some(), "missing {label}");
    }
}

/// Keeps each repo's latest board in memory, like GitHub does, so a test can
/// see whether edits reached the cloud before a board was reopened from it.
fn fake_github(repos: &[(&str, &Board)]) -> String {
    fake_github_on(std::net::TcpListener::bind("127.0.0.1:0").unwrap(), repos)
}

fn fake_github_on(listener: std::net::TcpListener, repos: &[(&str, &Board)]) -> String {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use std::io::{BufRead, BufReader, Read, Write};
    use storysquish::github::BOARD_FILE_NAME;

    let mut boards: std::collections::HashMap<String, String> = repos
        .iter()
        .map(|(repo, board)| ((*repo).to_owned(), serde_json::to_string(board).unwrap()))
        .collect();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let mut parts = request_line.split_whitespace();
            let method = parts.next().unwrap_or_default().to_owned();
            let path = parts.next().unwrap_or_default().to_owned();
            let mut content_length = 0;
            let mut wants_raw = false;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let line = line.trim_end();
                if line.is_empty() {
                    break;
                }
                let (name, value) = line.split_once(':').unwrap();
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = value.trim().parse().unwrap();
                }
                if name.eq_ignore_ascii_case("accept") {
                    wants_raw = value.contains("raw");
                }
            }
            let mut body = vec![0; content_length];
            reader.read_exact(&mut body).unwrap();

            let file_suffix = format!("/contents/{BOARD_FILE_NAME}");
            let not_found = (404, r#"{"message":"Not Found"}"#.to_owned());
            let (status, reply) = if path == "/user" {
                (200, r#"{"login":"someone"}"#.to_owned())
            } else if method == "POST" && path == "/user/repos" {
                let sent: serde_json::Value = serde_json::from_slice(&body).unwrap();
                let repo = format!("someone/{}", sent["name"].as_str().unwrap());
                let title = sent["description"]
                    .as_str()
                    .and_then(|d| d.strip_prefix("StorySquish board: "))
                    .unwrap_or_default();
                let created = Board {
                    title: title.to_owned(),
                    ..Board::default()
                };
                boards.insert(repo.clone(), serde_json::to_string(&created).unwrap());
                (201, repo_json(&repo, &boards[&repo]).to_string())
            } else if path.starts_with("/user/repos?") {
                let listed = boards
                    .iter()
                    .map(|(repo, content)| repo_json(repo, content))
                    .collect();
                (200, serde_json::Value::Array(listed).to_string())
            } else if let Some(repo) = path
                .strip_prefix("/repos/")
                .and_then(|rest| rest.strip_suffix(&file_suffix))
            {
                match method.as_str() {
                    _ if !boards.contains_key(repo) => not_found,
                    "PUT" => {
                        let sent: serde_json::Value = serde_json::from_slice(&body).unwrap();
                        let content = BASE64.decode(sent["content"].as_str().unwrap()).unwrap();
                        boards.insert(repo.to_owned(), String::from_utf8(content).unwrap());
                        (200, "{}".to_owned())
                    }
                    _ if wants_raw => (200, boards[repo].clone()),
                    _ => (200, r#"{"sha":"f00d"}"#.to_owned()),
                }
            } else {
                fake_repo_request(&mut boards, &method, &path, &body)
            };
            write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            )
            .unwrap();
        }
    });
    base
}

fn repo_json(repo: &str, content: &str) -> serde_json::Value {
    let board: Board = serde_json::from_str(content).unwrap();
    serde_json::json!({
        "full_name": repo,
        "private": true,
        "description": format!("StorySquish board: {}", board.title),
        "pushed_at": "2026-01-01T00:00:00Z",
    })
}

/// Reads the repo, or renames it when the PATCH carries a `name`.
fn fake_repo_request(
    boards: &mut std::collections::HashMap<String, String>,
    method: &str,
    path: &str,
    body: &[u8],
) -> (u16, String) {
    let repo = path.trim_start_matches("/repos/").to_owned();
    let sent: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
    let new_name = sent["name"]
        .as_str()
        .filter(|_| method == "PATCH" && boards.contains_key(&repo));
    if let (Some(name), Some((owner, _))) = (new_name, repo.split_once('/')) {
        let renamed = format!("{owner}/{name}");
        if boards.contains_key(&renamed) {
            return (
                422,
                r#"{"message":"name already exists on this account"}"#.to_owned(),
            );
        }
        let content = boards.remove(&repo).unwrap();
        let reply = repo_json(&renamed, &content).to_string();
        boards.insert(renamed, content);
        return (200, reply);
    }
    match boards.get(&repo) {
        Some(content) => (200, repo_json(&repo, content).to_string()),
        None => (404, r#"{"message":"Not Found"}"#.to_owned()),
    }
}

fn step_until(
    harness: &mut Harness<'_, PlannerApp>,
    done: impl Fn(&Harness<'_, PlannerApp>) -> bool,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !done(harness) {
        assert!(std::time::Instant::now() < deadline, "timed out");
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn open_from_cloud(harness: &mut Harness<'_, PlannerApp>, title: &str, path: &std::path::Path) {
    harness.get_by_label("Open").click();
    harness.run();
    harness.get_by_label("Cloud…").click();
    step_until(harness, |h| h.query_by_label(title).is_some());
    harness.get_by_label(title).click();
    step_until(harness, |h| h.state().board_path() == path);
    harness.run_steps(2);
}

#[test]
fn reopening_from_cloud_keeps_edits_made_just_before_switching_away() {
    let dir = tempfile::tempdir().unwrap();
    let settings_path = dir.path().join("settings.json");
    storysquish::github::save_token(&storysquish::github::token_path(&settings_path), "t").unwrap();
    let mut novel = Board {
        title: "Novel".to_owned(),
        ..Board::default()
    };
    novel.add_character("Ada");
    novel.add_character("Bo");
    let mut other = Board {
        title: "Other".to_owned(),
        ..Board::default()
    };
    other.add_character("Cy");
    let novel_path = dir.path().join("novel.json");
    let other_path = dir.path().join("other.json");
    storysquish::storage::save(&novel_path, &novel).unwrap();
    storysquish::storage::save(&other_path, &other).unwrap();
    let mut settings = storysquish::settings::Settings::default();
    settings.github_auto_backup = true;
    settings.set_repo(&novel_path, Some("someone/storysquish-novel".to_owned()));
    settings.set_repo(&other_path, Some("someone/storysquish-other".to_owned()));
    storysquish::settings::save(&settings_path, &settings).unwrap();
    let api_base = fake_github(&[
        ("someone/storysquish-novel", &novel),
        ("someone/storysquish-other", &other),
    ]);
    let open_path = novel_path.clone();
    let mut harness = Harness::builder()
        .with_size(WINDOW)
        .build_eframe(move |cc| {
            PlannerApp::new(cc, novel, open_path, settings_path, None)
                .with_github_api_base(&api_base)
        });
    harness.run();

    harness.get_by_label("Ada").click();
    harness.run();
    harness
        .get_by(|node| {
            node.role() == egui::accesskit::Role::ComboBox
                && node.label().as_deref() == Some("RELATIONSHIPS")
        })
        .click();
    harness.run();
    harness.get_all_by_label("Bo").last().unwrap().click();
    harness.run();
    harness.get_by_label("Add").click();
    harness.run();
    assert_eq!(harness.state().board().relationships.len(), 1);

    open_from_cloud(&mut harness, "Other", &other_path);
    open_from_cloud(&mut harness, "Novel", &novel_path);

    assert_eq!(harness.state().board().relationships.len(), 1);
    let saved = storysquish::storage::load(&novel_path).unwrap();
    assert_eq!(saved.relationships.len(), 1);
}

fn write_local_backup(dir: &std::path::Path, name: &str, titles: &[&str]) {
    storysquish::storage::save(
        &dir.join("board-backups").join(name),
        &board_with_scenes(titles),
    )
    .unwrap();
}

fn open_local_recovery(harness: &mut Harness<'_, PlannerApp>) {
    open_settings(harness);
    harness.get_by_label("Recover from local…").click();
    harness.run();
}

#[test]
fn recover_from_local_defaults_to_the_newest_readable_backup() {
    let Fixture { mut harness, dir } = launch(board_with_scenes(&["Current"]));
    write_local_backup(dir.path(), "board-001700000000000.json", &["Old", "Older"]);
    write_local_backup(dir.path(), "board-001800000000000.json", &["Newest"]);
    std::fs::write(
        dir.path().join("board-backups/board-001900000000000.json"),
        "{",
    )
    .unwrap();

    open_local_recovery(&mut harness);
    let unreadable = harness.get_by(|n| n.label().is_some_and(|l| l.ends_with("unreadable")));
    assert!(unreadable.accesskit_node().is_disabled());
    assert!(
        harness
            .query_by(|n| n
                .label()
                .is_some_and(|l| l.ends_with("Untitled Novel  ·  1 scene")))
            .is_some()
    );
    harness.get_by_label("Recover").click();
    harness.run();

    assert_eq!(scene_titles(&harness), ["Newest"]);
    assert!(harness.query_by_label("Recover").is_none());
}

#[test]
fn recover_from_local_restores_the_chosen_backup() {
    let Fixture { mut harness, dir } = launch(board_with_scenes(&["Current"]));
    write_local_backup(dir.path(), "board-001700000000000.json", &["Old", "Older"]);
    write_local_backup(
        dir.path(),
        "board-001800000000000-conflict.json",
        &["Theirs"],
    );

    open_local_recovery(&mut harness);
    assert!(
        harness
            .query_by(|n| n
                .label()
                .is_some_and(|l| l.ends_with("1 scene  ·  changed elsewhere")))
            .is_some()
    );
    harness
        .get_by(|n| n.label().is_some_and(|l| l.ends_with("2 scenes")))
        .click();
    harness.run();
    harness.get_by_label("Recover").click();
    harness.run();

    assert_eq!(scene_titles(&harness), ["Old", "Older"]);
}

#[test]
fn recover_from_local_keeps_a_copy_of_the_replaced_board() {
    let Fixture { mut harness, dir } = launch(board_with_scenes(&["Current"]));
    storysquish::storage::save(
        &dir.path().join("board.json"),
        &board_with_scenes(&["Current"]),
    )
    .unwrap();
    write_local_backup(dir.path(), "board-001700000000000.json", &["Old"]);

    open_local_recovery(&mut harness);
    harness.get_by_label("Recover").click();
    harness.run();

    assert_eq!(scene_titles(&harness), ["Old"]);
    let kept = std::fs::read_dir(dir.path().join("board-backups"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.to_string_lossy().ends_with("-before-recover.json"))
        .expect("the replaced board is kept as a backup");
    assert_eq!(
        storysquish::storage::load(&kept).unwrap().scenes[0].title,
        "Current"
    );
}

#[test]
fn recover_from_local_without_backups_says_so() {
    let Fixture { mut harness, .. } = launch(board_with_scenes(&["Current"]));

    open_local_recovery(&mut harness);

    assert!(
        harness
            .query_by_label("This board has no backups yet.")
            .is_some()
    );
    assert!(
        harness
            .get_by_label("Recover")
            .accesskit_node()
            .is_disabled()
    );
    harness.get_by_label("Cancel").click();
    harness.run();
    assert!(harness.query_by_label("Recover").is_none());
    assert!(harness.query_by_label("Recover from local…").is_some());
}

fn launch_with_fake_github(
    board: Board,
    linked_repo: Option<&str>,
    cloud: &[(&str, &Board)],
) -> (Harness<'static, PlannerApp>, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let settings_path = dir.path().join("settings.json");
    storysquish::github::save_token(&storysquish::github::token_path(&settings_path), "t").unwrap();
    let board_path = dir.path().join("board.json");
    storysquish::storage::save(&board_path, &board).unwrap();
    let mut settings = storysquish::settings::Settings::default();
    settings.set_repo(&board_path, linked_repo.map(str::to_owned));
    storysquish::settings::save(&settings_path, &settings).unwrap();
    let api_base = fake_github(cloud);
    let mut harness = Harness::builder()
        .with_size(WINDOW)
        .build_eframe(move |cc| {
            PlannerApp::new(cc, board, board_path, settings_path, None)
                .with_github_api_base(&api_base)
        });
    harness.run();
    (harness, dir)
}

/// As on first launch: a token, but no board file yet.
fn launch_first_time_with_fake_github() -> (Harness<'static, PlannerApp>, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let settings_path = dir.path().join("settings.json");
    storysquish::github::save_token(&storysquish::github::token_path(&settings_path), "t").unwrap();
    let board_path = dir.path().join("board.json");
    let api_base = fake_github(&[]);
    let mut harness = Harness::builder()
        .with_size(WINDOW)
        .build_eframe(move |cc| {
            PlannerApp::new(cc, Board::default(), board_path, settings_path, None)
                .with_github_api_base(&api_base)
                .offering_repo_on_first_change()
        });
    harness.run();
    (harness, dir)
}

fn titled_board(title: &str) -> Board {
    Board {
        title: title.to_owned(),
        ..board_with_scenes(&["Opening"])
    }
}

fn repo_name_field<'h>(harness: &'h Harness<'_, PlannerApp>) -> egui_kittest::Node<'h> {
    labelled_text_input(harness, "REPOSITORY NAME")
}

fn repo_name_shown(harness: &Harness<'_, PlannerApp>) -> Option<String> {
    harness
        .query_by(|node| {
            node.role() == egui::accesskit::Role::TextInput
                && node.label().as_deref() == Some("REPOSITORY NAME")
        })
        .and_then(|node| node.value())
}

fn linked_repo(harness: &Harness<'_, PlannerApp>) -> Option<String> {
    harness
        .state()
        .settings()
        .repo(harness.state().board_path())
        .map(str::to_owned)
}

#[test]
fn backing_up_without_a_repo_asks_before_creating_one_with_a_free_name() {
    let taken = Board::default();
    let (mut harness, _dir) = launch_with_fake_github(
        titled_board("Dune"),
        None,
        &[("someone/storysquish-dune", &taken)],
    );
    open_settings(&mut harness);
    harness.get_by_label("Back up now").click();
    step_until(&mut harness, |h| {
        repo_name_shown(h).as_deref() == Some("storysquish-dune-2")
    });
    assert_eq!(linked_repo(&harness), None);

    harness.get_by_label("Create").click();
    step_until(&mut harness, |h| {
        h.query_by_label_contains("Backed up").is_some()
    });

    assert_eq!(
        linked_repo(&harness).as_deref(),
        Some("someone/storysquish-dune-2")
    );
    assert!(repo_name_shown(&harness).is_none());
}

#[test]
fn the_suggested_repo_name_can_be_changed_before_creating() {
    let (mut harness, _dir) = launch_with_fake_github(titled_board("Dune"), None, &[]);
    open_settings(&mut harness);
    harness.get_by_label("Back up now").click();
    step_until(&mut harness, |h| {
        repo_name_shown(h).as_deref() == Some("storysquish-dune")
            && h.query_by_label("Checking which names are free…").is_none()
    });

    repo_name_field(&harness).focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    repo_name_field(&harness).type_text("arrakis");
    harness.run();
    harness.get_by_label("Create").click();
    step_until(&mut harness, |h| linked_repo(h).is_some());

    assert_eq!(linked_repo(&harness).as_deref(), Some("someone/arrakis"));
}

#[test]
fn cancelling_repo_creation_leaves_the_board_unlinked() {
    let (mut harness, _dir) = launch_with_fake_github(titled_board("Dune"), None, &[]);
    open_settings(&mut harness);
    harness.get_by_label("Back up now").click();
    step_until(&mut harness, |h| repo_name_shown(h).is_some());

    harness.get_by_label("Cancel").click();
    harness.run();

    assert!(repo_name_shown(&harness).is_none());
    assert_eq!(linked_repo(&harness), None);
}

#[test]
fn a_linked_repo_that_no_longer_exists_offers_to_create_it_again() {
    let (mut harness, _dir) =
        launch_with_fake_github(titled_board("Dune"), Some("someone/dune-gone"), &[]);
    open_settings(&mut harness);
    harness.get_by_label("Back up now").click();
    step_until(&mut harness, |h| {
        h.query_by_label_contains("someone/dune-gone does not exist")
            .is_some()
    });
    assert_eq!(repo_name_shown(&harness).as_deref(), Some("dune-gone"));

    harness.get_by_label("Create").click();
    step_until(&mut harness, |h| {
        h.query_by_label_contains("Backed up").is_some()
    });
    assert_eq!(linked_repo(&harness).as_deref(), Some("someone/dune-gone"));
}

#[test]
fn opening_a_board_adds_the_prefix_to_its_repo_name() {
    let dune = titled_board("Dune");
    let (mut harness, dir) = launch_with_fake_github(
        dune.clone(),
        Some("someone/dune"),
        &[("someone/dune", &dune)],
    );

    step_until(&mut harness, |h| {
        linked_repo(h).as_deref() == Some("someone/storysquish-dune")
    });

    let saved = storysquish::settings::load(&dir.path().join("settings.json")).unwrap();
    assert_eq!(
        saved.repo(&dir.path().join("Dune.json")),
        Some("someone/storysquish-dune")
    );
}

#[test]
fn a_new_title_renames_the_repo_on_the_next_backup() {
    let dune = titled_board("Dune");
    let (mut harness, _dir) = launch_with_fake_github(
        dune.clone(),
        Some("someone/storysquish-dune"),
        &[("someone/storysquish-dune", &dune)],
    );

    harness.get_all_by_value("Dune").next().unwrap().focus();
    harness.run();
    harness.event(egui::Event::Text(" Messiah".to_owned()));
    harness.run();
    open_settings(&mut harness);
    harness.get_by_label("Back up now").click();
    step_until(&mut harness, |h| {
        linked_repo(h).as_deref() == Some("someone/storysquish-dune-messiah")
    });
}

fn header_title_field<'h>(harness: &'h Harness<'_, PlannerApp>) -> egui_kittest::Node<'h> {
    harness.get_by(|node| {
        node.role() == egui::accesskit::Role::TextInput
            && node.value().as_deref() == Some("Untitled Novel")
    })
}

fn story_title_field<'h>(harness: &'h Harness<'_, PlannerApp>) -> egui_kittest::Node<'h> {
    labelled_text_input(harness, "STORY TITLE")
}

#[test]
fn the_first_change_to_a_new_board_asks_for_a_title_and_a_repo() {
    let (mut harness, _dir) = launch_with_fake_github(titled_board("Dune"), None, &[]);
    harness.get_by_label("New").click();
    harness.run();
    assert!(repo_name_shown(&harness).is_none());

    harness.get_by_label("+ Scene").click();
    step_until(&mut harness, |h| {
        h.query_by_label("someone /").is_some()
            && h.query_by_label("Checking which names are free…").is_none()
    });
    assert_eq!(
        repo_name_shown(&harness).as_deref(),
        Some("storysquish-board")
    );
    assert!(
        harness
            .get_by_label("Create")
            .accesskit_node()
            .is_disabled()
    );

    story_title_field(&harness).type_text("Children of Dune");
    harness.run_steps(2);
    step_until(&mut harness, |h| {
        repo_name_shown(h).as_deref() == Some("storysquish-children-of-dune")
            && h.query_by_label("Checking which names are free…").is_none()
    });
    harness.get_by_label("Create").click();
    step_until(&mut harness, |h| linked_repo(h).is_some());

    assert_eq!(harness.state().board().title, "Children of Dune");
    assert_eq!(
        linked_repo(&harness).as_deref(),
        Some("someone/storysquish-children-of-dune")
    );
}

#[test]
fn a_new_board_titled_in_the_header_is_offered_a_repo_named_after_it() {
    let (mut harness, _dir) = launch_with_fake_github(titled_board("Dune"), None, &[]);
    harness.get_by_label("New").click();
    harness.run();

    let header_title = harness.get_by(|node| {
        node.role() == egui::accesskit::Role::TextInput
            && node.value().as_deref() == Some("Untitled Novel")
    });
    header_title.focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by(|node| {
            node.role() == egui::accesskit::Role::TextInput
                && node.value().as_deref() == Some("Untitled Novel")
        })
        .type_text("Arrakis");
    harness.run();
    assert!(repo_name_shown(&harness).is_none());

    harness.get_by_label("+ Scene").click();
    step_until(&mut harness, |h| {
        repo_name_shown(h).as_deref() == Some("storysquish-arrakis")
    });
    assert!(harness.query_by_label("STORY TITLE").is_none());
}

#[test]
fn a_new_board_is_not_offered_a_repo_without_a_token() {
    let Fixture { mut harness, .. } = launch(board_with_scenes(&["Opening"]));
    harness.get_by_label("New").click();
    harness.run();

    harness.get_by_label("+ Scene").click();
    harness.run();

    assert!(repo_name_shown(&harness).is_none());
    assert_eq!(harness.state().board().scenes.len(), 1);
}

#[test]
fn the_first_change_on_first_launch_offers_a_repo() {
    let (mut harness, _dir) = launch_first_time_with_fake_github();
    assert!(repo_name_shown(&harness).is_none());

    harness.get_by_label("+ Scene").click();
    step_until(&mut harness, |h| {
        h.query_by_label("someone /").is_some()
            && h.query_by_label("Checking which names are free…").is_none()
    });

    assert!(harness.query_by_label("STORY TITLE").is_some());
    assert_eq!(
        repo_name_shown(&harness).as_deref(),
        Some("storysquish-board")
    );
}

#[test]
fn a_repo_created_for_an_empty_board_still_gets_the_board_file() {
    let (mut harness, _dir) = launch_with_fake_github(titled_board("Dune"), None, &[]);
    harness.get_by_label("New").click();
    harness.run();
    header_title_field(&harness).focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    header_title_field(&harness).type_text("Arrakis");
    harness.run();
    harness.key_press(egui::Key::Tab);
    step_until(&mut harness, |h| {
        repo_name_shown(h).as_deref() == Some("storysquish-arrakis")
            && h.query_by_label("Checking which names are free…").is_none()
    });

    harness.get_by_label("Create").click();
    step_until(&mut harness, |h| linked_repo(h).is_some());
    open_settings(&mut harness);
    step_until(&mut harness, |h| {
        h.query_by_label_contains("Backed up").is_some()
    });

    assert!(harness.state().board().is_empty());
    assert_eq!(
        linked_repo(&harness).as_deref(),
        Some("someone/storysquish-arrakis")
    );
}

/// A board linked to `REPO_DUNE` with automatic backup on, talking to
/// `api_base`, plus a second linked board `other.json` that is not open.
fn launch_auto_backup(
    api_base: String,
    configure: impl FnOnce(&mut storysquish::settings::Settings, &std::path::Path),
) -> (Harness<'static, PlannerApp>, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let settings_path = dir.path().join("settings.json");
    storysquish::github::save_token(&storysquish::github::token_path(&settings_path), "t").unwrap();
    let board_path = dir.path().join("Dune.json");
    let dune = titled_board("Dune");
    storysquish::storage::save(&board_path, &dune).unwrap();
    let other_path = dir.path().join("other.json");
    storysquish::storage::save(&other_path, &titled_board("Other")).unwrap();
    let mut settings = storysquish::settings::Settings::default();
    settings.github_auto_backup = true;
    settings.set_repo(&board_path, Some(REPO_DUNE.to_owned()));
    settings.set_repo(&other_path, Some(REPO_OTHER.to_owned()));
    configure(&mut settings, dir.path());
    storysquish::settings::save(&settings_path, &settings).unwrap();
    let mut harness = Harness::builder()
        .with_size(WINDOW)
        .build_eframe(move |cc| {
            PlannerApp::new(cc, dune, board_path, settings_path, None)
                .with_github_api_base(&api_base)
        });
    harness.run();
    (harness, dir)
}

const REPO_DUNE: &str = "someone/storysquish-dune";
const REPO_OTHER: &str = "someone/storysquish-other";

fn saved_unpushed(dir: &TempDir, board: &str) -> bool {
    storysquish::settings::load(&dir.path().join("settings.json"))
        .unwrap()
        .is_unpushed(&dir.path().join(board))
}

fn cloud_board(api_base: &str, repo: &str) -> Board {
    let json = storysquish::github::Client::with_api_base("t", api_base)
        .latest_board(repo)
        .unwrap();
    serde_json::from_str(&json).unwrap()
}

fn set_window_focus(harness: &mut Harness<'_, PlannerApp>, focused: bool) {
    harness.input_mut().focused = focused;
    harness.event(egui::Event::WindowFocused(focused));
    harness.run();
}

fn unused_local_address() -> std::net::SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

#[test]
fn an_offline_backup_stays_quiet_and_resumes_when_github_returns() {
    let address = unused_local_address();
    let (mut harness, dir) = launch_auto_backup(format!("http://{address}"), |_, _| {});

    harness.get_by_label("+ Scene").click();
    harness.run();
    set_window_focus(&mut harness, false);
    step_until(&mut harness, |h| h.query_by_label("offline").is_some());

    assert!(
        harness
            .query_by(|n| n
                .label()
                .is_some_and(|l| l.contains("GitHub backup failed")))
            .is_none()
    );
    assert!(saved_unpushed(&dir, "Dune.json"));

    let dune = titled_board("Dune");
    let api_base = fake_github_on(
        std::net::TcpListener::bind(address).unwrap(),
        &[(REPO_DUNE, &dune), (REPO_OTHER, &titled_board("Other"))],
    );
    set_window_focus(&mut harness, true);
    step_until(&mut harness, |h| h.query_by_label("offline").is_none());
    step_until(&mut harness, |_| !saved_unpushed(&dir, "Dune.json"));

    assert_eq!(cloud_board(&api_base, REPO_DUNE).scenes.len(), 2);
}

#[test]
fn edits_left_unbacked_by_an_earlier_session_are_backed_up_on_launch() {
    let mut cloud_dune = titled_board("Dune");
    cloud_dune.scenes.clear();
    let api_base = fake_github(&[(REPO_DUNE, &cloud_dune), (REPO_OTHER, &Board::default())]);
    let (mut harness, dir) = launch_auto_backup(api_base.clone(), |settings, dir| {
        settings.set_unpushed(&dir.join("Dune.json"), true);
        settings.set_unpushed(&dir.join("other.json"), true);
    });

    step_until(&mut harness, |_| {
        !saved_unpushed(&dir, "Dune.json") && !saved_unpushed(&dir, "other.json")
    });

    assert_eq!(cloud_board(&api_base, REPO_DUNE).scenes.len(), 1);
    assert_eq!(cloud_board(&api_base, REPO_OTHER).title, "Other");
}

#[test]
fn closing_on_a_network_that_never_answers_gives_up_and_flags_the_board() {
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let api_base = format!("http://{}", silent.local_addr().unwrap());
    let (mut harness, dir) = launch_auto_backup(api_base, |_, _| {});

    harness.get_by_label("+ Scene").click();
    harness.run();
    set_window_focus(&mut harness, false);
    harness.run_steps(2);

    let started = std::time::Instant::now();
    eframe::App::on_exit(harness.state_mut());

    assert!(started.elapsed() < std::time::Duration::from_secs(15));
    assert!(saved_unpushed(&dir, "Dune.json"));
    drop(silent);
}
