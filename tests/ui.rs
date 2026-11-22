use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
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
    let notes_role = egui::accesskit::Role::MultilineTextInput;
    harness.get_by_role(notes_role).focus();
    harness.run();
    harness.get_by_role(notes_role).type_text("Wants revenge");
    harness.run();

    assert_eq!(harness.state().board().characters[0].notes, "Wants revenge");
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
    storysquish::settings::save(
        &settings_path,
        &storysquish::settings::Settings {
            color_scheme: ColorScheme::Ember,
        },
    )
    .unwrap();
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
