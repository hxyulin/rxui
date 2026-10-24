use crate::*;
use std::cell::Cell;
use std::rc::Rc;

/// Measures 8 units per char and records the spans of rich text.
#[derive(Default)]
struct Measure {
    spans: std::collections::HashMap<String, Vec<ResolvedSpan>>,
}
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        if !r.spans.is_empty() {
            self.spans.insert(r.text.into(), r.spans.to_vec());
        }
        Ok([r.text.chars().count() as f32 * 8., 16.])
    }
}

/// Fuzzy matching that counts its calls.
struct Counting(Rc<Cell<usize>>);
impl Matcher for Counting {
    fn score(&self, query: &str, candidate: &str) -> Option<Match> {
        self.0.set(self.0.get() + 1);
        FuzzyMatcher.score(query, candidate)
    }
}

struct Files {
    query: String,
    files: Vec<String>,
    inline: bool,
    calls: Rc<Cell<usize>>,
    chosen: Vec<usize>,
    queries: Vec<String>,
    /// Unrelated state that redescribes the view.
    other: u32,
}
impl Files {
    fn new(files: &[&str]) -> Self {
        Self {
            query: String::new(),
            files: files.iter().map(|f| f.to_string()).collect(),
            inline: false,
            calls: Rc::default(),
            chosen: Vec::new(),
            queries: Vec::new(),
            other: 0,
        }
    }
}
impl View for Files {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut combo = combobox(cx)
            .key("combo")
            .accessibility_label("Go to file")
            .query(&self.query)
            .on_query(cx.listener(|s, e: &QueryEvent, _| {
                s.query = e.text.clone();
                s.queries.push(e.text.clone());
            }))
            .items(self.files.len(), |i| {
                ComboItem::new(&self.files[i])
                    .icon(Icon::FILE)
                    .detail("src")
            })
            .matcher(Counting(self.calls.clone()))
            .on_choose(cx.listener(|s, e: &ChooseEvent, _| s.chosen.push(e.index)));
        if self.inline {
            combo = combo.inline();
        }
        column()
            .padding(10.)
            .child(label(format!("{}", self.other)))
            .child(combo)
    }
}

struct Harness {
    r: Runtime,
    e: Entity<Files>,
    ui: Ui<Files>,
    measure: Measure,
}
impl Harness {
    fn new(files: Files) -> Self {
        let mut r = Runtime::new();
        let e = r.update(|cx| cx.new(|_| files));
        let ui = Ui::new(&mut r, e.clone()).unwrap();
        let mut h = Self {
            r,
            e,
            ui,
            measure: Measure::default(),
        };
        h.prepare();
        h.ui.focus(h.id("input"));
        h.prepare();
        h
    }
    fn prepare(&mut self) {
        // The list measures its viewport before describing rows.
        for _ in 0..3 {
            self.ui
                .prepare(&mut self.r, [600., 600.], &mut self.measure)
                .unwrap();
        }
    }
    fn update(&mut self, f: impl FnOnce(&mut Files)) {
        self.r.update(|cx| self.e.update(cx, |s, _| f(s)));
        self.prepare();
    }
    fn read<R>(&mut self, f: impl FnOnce(&Files) -> R) -> R {
        let e = self.e.clone();
        self.r.update(|cx| f(&e.read(cx)))
    }
    fn find(&self, key: impl Into<Key>) -> Option<ElementId> {
        let key = key.into();
        self.ui
            .elements()
            .find(|e| e.key == Some(&key))
            .map(|e| e.id)
    }
    fn id(&self, key: impl Into<Key>) -> ElementId {
        let key = key.into();
        self.find(key.clone())
            .unwrap_or_else(|| panic!("no element {key:?}"))
    }
    fn key_with(&mut self, key: KeyboardKey, modifiers: Modifiers) -> bool {
        let event = KeyEvent {
            physical_key: Default::default(),
            key,
            pressed: true,
            repeat: false,
            modifiers,
        };
        let result = self.ui.key(&mut self.r, event).unwrap();
        self.prepare();
        result.default_prevented
    }
    fn key(&mut self, key: KeyboardKey) -> bool {
        self.key_with(key, Modifiers::default())
    }
    fn type_text(&mut self, text: &str) {
        self.ui
            .text_input(
                &mut self.r,
                TextInputEvent::Insert(text.into()),
                &mut self.measure,
            )
            .unwrap();
        self.prepare();
    }
    /// Labels of the mounted options, in result order.
    fn options(&self) -> Vec<String> {
        let mut options: Vec<_> = self
            .ui
            .semantics()
            .filter(|n| n.role == SemanticRole::ListBoxOption)
            .map(|n| (n.position_in_set.unwrap(), n.label.unwrap().to_string()))
            .collect();
        options.sort();
        options.into_iter().map(|(_, label)| label).collect()
    }
    /// The highlighted option's label, through the input's active descendant.
    fn highlighted(&self) -> Option<String> {
        let input = self.ui.semantic_node(self.id("input")).unwrap();
        let active = input.active_descendant?;
        Some(
            self.ui
                .semantic_node(active)
                .unwrap()
                .label
                .unwrap()
                .to_string(),
        )
    }
    fn option(&self, label: &str) -> SemanticNode<'_> {
        self.ui
            .semantics()
            .find(|n| n.role == SemanticRole::ListBoxOption && n.label == Some(label))
            .unwrap()
    }
    fn open(&self) -> bool {
        self.find("list").is_some()
    }
}

/// Labels of `FILES` at `indices`.
fn named(indices: &[usize]) -> Vec<String> {
    indices.iter().map(|i| FILES[*i].to_string()).collect()
}

const FILES: [&str; 6] = [
    "path/to/the_main.rs",
    "themes/mod.rs",
    "src/math.rs",
    "theme.rs",
    "docs/theming.md",
    "README.md",
];

#[test]
fn typing_opens_ranked_results_and_enter_chooses_by_item_index() {
    let mut h = Harness::new(Files::new(&FILES));
    assert!(!h.open());
    let input = h.ui.semantic_node(h.id("input")).unwrap();
    assert_eq!(input.role, SemanticRole::ComboBox);
    assert_eq!(input.label, Some("Go to file"));
    assert_eq!(input.expanded, Some(false));
    assert_eq!(input.active_descendant, None);

    h.type_text("thm");
    assert_eq!(h.read(|s| s.queries.clone()), ["thm"]);
    assert!(h.open());
    assert_eq!(h.options(), named(&[3, 1, 0, 4]));
    assert_eq!(h.highlighted().as_deref(), Some("theme.rs"));
    let input = h.ui.semantic_node(h.id("input")).unwrap();
    assert_eq!(input.expanded, Some(true));
    let list = h.ui.semantic_node(h.id("list")).unwrap();
    assert_eq!(list.role, SemanticRole::ListBox);
    assert_eq!(list.label, Some("Go to file"));
    assert_eq!(input.controls, Some(list.id));
    let option = h.option("themes/mod.rs");
    assert_eq!(option.role, SemanticRole::ListBoxOption);
    assert_eq!(
        (option.position_in_set, option.set_size),
        (Some(1), Some(4))
    );
    assert_eq!(option.selected, Some(false));
    // Focus stays in the input.
    assert_eq!(h.ui.focused_element(), Some(h.id("input")));

    // Down, then Enter, chooses the second result's original index.
    assert!(h.key(KeyboardKey::ArrowDown));
    assert_eq!(h.highlighted().as_deref(), Some("themes/mod.rs"));
    assert_eq!(h.option("themes/mod.rs").selected, Some(true));
    assert!(h.key(KeyboardKey::Enter));
    assert_eq!(h.read(|s| s.chosen.clone()), [1]);
    assert!(!h.open());
    // Enter in a closed popover is the input's.
    assert!(!h.key(KeyboardKey::Enter));
    assert_eq!(h.read(|s| s.chosen.clone()), [1]);
}

#[test]
fn arrows_wrap_pages_move_and_primary_home_end_reach_the_ends() {
    let files: Vec<String> = (0..40).map(|i| format!("file_{i:02}.rs")).collect();
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    let mut h = Harness::new(Files::new(&refs));
    let lit = |h: &Harness| h.highlighted().unwrap();
    // Down opens the closed list on the first item.
    assert!(h.key(KeyboardKey::ArrowDown));
    assert!(h.open());
    assert_eq!(lit(&h), "file_00.rs");
    assert!(h.key(KeyboardKey::ArrowUp));
    assert_eq!(lit(&h), "file_39.rs");
    assert!(h.key(KeyboardKey::ArrowDown));
    assert_eq!(lit(&h), "file_00.rs");
    // A page is the list's 10 visible rows; pages stop at the ends.
    assert!(h.key(KeyboardKey::PageDown));
    assert_eq!(lit(&h), "file_10.rs");
    assert!(h.key(KeyboardKey::PageUp));
    assert!(h.key(KeyboardKey::PageUp));
    assert_eq!(lit(&h), "file_00.rs");
    // Home and End belong to the caret; Primary+Home/End move the highlight.
    assert!(!h.key(KeyboardKey::End));
    assert_eq!(lit(&h), "file_00.rs");
    let primary = if cfg!(target_os = "macos") {
        Modifiers {
            meta: true,
            ..Default::default()
        }
    } else {
        Modifiers {
            control: true,
            ..Default::default()
        }
    };
    assert!(h.key_with(KeyboardKey::End, primary));
    assert_eq!(lit(&h), "file_39.rs");
    // The highlight scrolled into view.
    assert!(h.find(39).is_some());
    assert!(h.find(0).is_none());
    assert!(h.key_with(KeyboardKey::Home, primary));
    assert_eq!(lit(&h), "file_00.rs");
    assert!(h.find(0).is_some());
}

#[test]
fn escape_closes_an_open_popover_and_is_left_unhandled_when_closed() {
    let mut h = Harness::new(Files::new(&FILES));
    h.type_text("the");
    assert!(h.open());
    assert!(h.key(KeyboardKey::Escape));
    assert!(!h.open());
    assert!(!h.key(KeyboardKey::Escape));
    // Typing reopens it.
    h.type_text("m");
    assert!(h.open());
    // Inline results stay visible and leave Escape to a dialog.
    h.update(|s| s.inline = true);
    assert!(!h.key(KeyboardKey::Escape));
    assert!(h.open());
}

#[test]
fn the_highlight_resets_on_query_changes_and_follows_its_item_otherwise() {
    let mut h = Harness::new(Files::new(&FILES));
    h.type_text("th");
    assert!(h.key(KeyboardKey::ArrowDown));
    let second = h.highlighted().unwrap();
    // A new item that ranks first: the highlighted item keeps the highlight at
    // its new position.
    h.update(|s| s.files.push("th.rs".into()));
    assert_eq!(h.options()[0], "th.rs");
    assert_eq!(h.highlighted().unwrap(), second);
    // An unrelated rescoring keeps it too.
    h.update(|s| s.files.push("zzz".into()));
    assert_eq!(h.highlighted().unwrap(), second);
    // A changed query starts on the first result again.
    h.type_text("m");
    assert_eq!(h.highlighted().as_deref(), Some("theme.rs"));
    // So does a query the application changes.
    h.key(KeyboardKey::ArrowDown);
    h.update(|s| s.query = "math".into());
    assert_eq!(h.highlighted().as_deref(), Some("src/math.rs"));
    // A highlighted item that no longer matches moves the highlight to the
    // first result.
    h.update(|s| s.query = "th".into());
    h.key(KeyboardKey::ArrowDown);
    let lit = h.highlighted().unwrap();
    h.update(|s| {
        let index = s.files.iter().position(|f| *f == lit).unwrap();
        s.files[index] = "zzz".into();
    });
    assert_eq!(h.highlighted().as_deref(), Some("th.rs"));
}

#[test]
fn hover_highlights_and_clicks_choose_without_moving_focus() {
    let mut h = Harness::new(Files::new(&FILES));
    h.type_text("thm");
    let b = h.ui.element(h.id(0)).unwrap().bounds;
    let at = [b.x + 4., b.y + b.height / 2.];
    h.ui.pointer(&mut h.r, PointerEvent::Moved(at)).unwrap();
    h.prepare();
    assert_eq!(h.highlighted().as_deref(), Some("path/to/the_main.rs"));
    h.ui.pointer(&mut h.r, PointerEvent::Pressed(at)).unwrap();
    h.prepare();
    assert_eq!(h.ui.focused_element(), Some(h.id("input")));
    h.ui.pointer(&mut h.r, PointerEvent::Released(at)).unwrap();
    h.prepare();
    assert_eq!(h.read(|s| s.chosen.clone()), [0]);
    assert!(!h.open());
}

#[test]
fn no_results_show_the_empty_text() {
    let mut h = Harness::new(Files::new(&FILES));
    h.type_text("qqq");
    assert!(h.open());
    assert!(h.options().is_empty());
    let empty = h.ui.element(h.id("empty")).unwrap();
    let text =
        h.ui.elements()
            .find(|e| e.parent == Some(empty.id))
            .unwrap();
    assert_eq!(text.text, Some("No results"));
    assert_eq!(text.color, Theme::dark().palette().text_muted);
    assert_eq!(h.highlighted(), None);
    // Enter has nothing to choose.
    assert!(!h.key(KeyboardKey::Enter));
    assert!(h.read(|s| s.chosen.is_empty()));
}

#[test]
fn inline_results_show_every_item_in_flow_below_the_input() {
    let mut files = Files::new(&FILES);
    files.inline = true;
    let mut h = Harness::new(files);
    assert!(h.open());
    // An empty query lists every item in order.
    assert_eq!(h.options(), named(&[0, 1, 2, 3, 4, 5]));
    let input = h.ui.element(h.id("input")).unwrap().bounds;
    let list = h.ui.element(h.id("list")).unwrap();
    assert!(!list.viewport_overlay);
    assert!(list.bounds.y >= input.y + input.height);
    let m = ThemeMetrics::balanced();
    assert_eq!(list.bounds.height, 6. * m.row_height);
    assert_eq!(h.highlighted().as_deref(), Some(FILES[0]));
    assert!(h.key(KeyboardKey::Enter));
    assert_eq!(h.read(|s| s.chosen.clone()), [0]);
    assert!(h.open());
}

#[test]
fn matched_characters_paint_in_the_focus_color_at_medium_weight() {
    let mut h = Harness::new(Files::new(&[
        "theme.rs",
        "themes/mod.rs",
        "日本語/ファイル.rs",
    ]));
    h.type_text("thm");
    let focus = Theme::dark().palette().focus;
    let spans: Vec<_> = h.measure.spans["themes/mod.rs"]
        .iter()
        .map(|s| (s.range.clone(), s.color, s.weight))
        .collect();
    let lit = (Some(focus), Some(FontWeight::MEDIUM));
    assert_eq!(spans, [(0..2, lit.0, lit.1), (7..8, lit.0, lit.1)]);
    // The highlighted row keeps the text color, which stays readable over its fill.
    let spans: Vec<_> = h.measure.spans["theme.rs"]
        .iter()
        .map(|s| (s.range.clone(), s.color, s.weight))
        .collect();
    let bold = Some(FontWeight::MEDIUM);
    assert_eq!(spans, [(0..2, None, bold), (3..4, None, bold)]);
    // Ranges are byte ranges on char boundaries.
    h.update(|s| s.query = "本フ".into());
    let spans: Vec<_> = h.measure.spans["日本語/ファイル.rs"]
        .iter()
        .filter(|s| s.weight == Some(FontWeight::MEDIUM))
        .map(|s| s.range.clone())
        .collect();
    assert_eq!(spans, [3..6, 10..13]);
}

#[test]
fn fifty_thousand_items_mount_only_visible_rows_and_score_once_per_query() {
    let files: Vec<String> = (0..50_000)
        .map(|i| format!("src/module_{i}/file_{i}.rs"))
        .collect();
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    let mut h = Harness::new(Files::new(&refs));
    h.type_text("f");
    assert_eq!(h.read(|s| s.calls.get()), 50_000);
    let options = h.options().len();
    assert!((10..=16).contains(&options), "{options} rows mounted");
    // Redescribing for unrelated state does not score again.
    h.update(|s| s.other += 1);
    h.update(|s| s.other += 1);
    assert_eq!(h.read(|s| s.calls.get()), 50_000);
    // Moving the highlight does not either.
    h.key(KeyboardKey::ArrowDown);
    assert_eq!(h.read(|s| s.calls.get()), 50_000);
    // A new query does.
    h.type_text("1");
    assert_eq!(h.read(|s| s.calls.get()), 100_000);
    // So do changed labels for the same query.
    h.update(|s| s.files[0] = "f1.rs".into());
    assert_eq!(h.read(|s| s.calls.get()), 150_000);
}
