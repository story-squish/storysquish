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

fn gist_field<'h>(harness: &'h Harness<'_, PlannerApp>) -> egui_kittest::Node<'h> {
    labelled_text_input(harness, "GITHUB GIST")
}

#[test]
fn connecting_github_stores_the_token_privately_and_disconnecting_forgets_it() {
    let Fixture { mut harness, dir } = launch(board_with_scenes(&["Opening"]));
    let token_path = dir.path().join("config/storysquish/github-token");
    open_settings(&mut harness);

    gist_field(&harness).focus();
    harness.run();
    gist_field(&harness).type_text("ghp_example");
    harness.run();
    harness.get_by_label("Connect").click();
    harness.run();

    assert_eq!(
        storysquish::gist::load_token(&token_path)
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
            .query_by_label("The first backup creates a secret gist.")
            .is_some()
    );

    harness.get_by_label("Disconnect").click();
    harness.run();
    assert_eq!(storysquish::gist::load_token(&token_path).unwrap(), None);
    assert!(harness.query_by_label("Connect").is_some());
}

fn launch_connected(board: Board) -> (Harness<'static, PlannerApp>, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let settings_path = dir.path().join("settings.json");
    storysquish::gist::save_token(&storysquish::gist::token_path(&settings_path), "t").unwrap();
    let board_path = dir.path().join("board.json");
    let mut harness = Harness::builder()
        .with_size(WINDOW)
        .build_eframe(move |cc| PlannerApp::new(cc, board, board_path, settings_path, None));
    harness.run();
    (harness, dir)
}

#[test]
fn pasting_a_gist_url_stores_its_id() {
    let (mut harness, dir) = launch_connected(Board::default());
    open_settings(&mut harness);

    gist_field(&harness).focus();
    harness.run();
    gist_field(&harness).type_text("https://gist.github.com/someone/aa5a315d61ae9438b18d");
    harness.run();
    assert_eq!(
        harness
            .state()
            .settings()
            .gist_id(harness.state().board_path()),
        Some("aa5a315d61ae9438b18d")
    );
    assert!(harness.query_by_label("Restore from gist…").is_some());

    harness.get_by_label("Back up automatically").click();
    harness.run();
    let saved = storysquish::settings::load(&dir.path().join("settings.json")).unwrap();
    assert_eq!(
        saved.gist_id(&dir.path().join("board.json")),
        Some("aa5a315d61ae9438b18d")
    );
    assert!(saved.gist_auto_backup);
}

#[test]
fn reset_unlinks_the_boards_gist() {
    let (mut harness, dir) = launch_connected(board_with_scenes(&["Opening"]));
    open_settings(&mut harness);
    gist_field(&harness).focus();
    harness.run();
    gist_field(&harness).type_text("aa5a315d61ae9438b18d");
    harness.run();
    harness.key_press(egui::Key::Escape);
    harness.run();

    harness.get_by_label("Reset").click();
    harness.run();
    harness.get_by_label("Delete").click();
    harness.run();

    assert!(harness.state().board().is_empty());
    assert_eq!(
        harness
            .state()
            .settings()
            .gist_id(harness.state().board_path()),
        None
    );
    let saved = storysquish::settings::load(&dir.path().join("settings.json")).unwrap();
    assert_eq!(saved.gist_id(&dir.path().join("board.json")), None);
}

#[test]
fn text_that_is_not_a_gist_is_flagged_and_not_saved() {
    let (mut harness, _dir) = launch_connected(Board::default());
    open_settings(&mut harness);

    gist_field(&harness).focus();
    harness.run();
    gist_field(&harness).type_text("my novel");
    harness.run();

    assert!(harness.query_by_label("Not a gist id or URL").is_some());
    assert_eq!(
        harness
            .state()
            .settings()
            .gist_id(harness.state().board_path()),
        None
    );
}

#[test]
fn empty_board_is_not_backed_up_to_gist() {
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
fn open_offers_only_cloud_boards_with_a_github_connection() {
    let (harness, _dir) = launch_connected(Board::default());
    assert!(harness.query_by_label("Local").is_none());
    assert!(harness.query_by_label("Cloud…").is_none());
    assert!(!harness.get_by_label("Open").accesskit_node().is_disabled());
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
