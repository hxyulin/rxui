use super::forms_tests::H;
use crate::*;
use std::time::Duration;

struct Banner {
    tone: Tone,
    dismissed: u32,
}
impl View for Banner {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column().fill_width().padding(10.).child(
            callout(self.tone, "Out of date")
                .key("callout")
                .description("Update to continue.")
                .action(button("Update").key("update"))
                .on_dismiss(cx.listener(|s, _: &ClickEvent, _| s.dismissed += 1)),
        )
    }
}

const TONES: [Tone; 5] = [
    Tone::Neutral,
    Tone::Accent,
    Tone::Danger,
    Tone::Warning,
    Tone::Success,
];

#[test]
fn callouts_paint_their_tone_and_choose_icon_and_role() {
    use super::feedback::tone_icon;
    assert_eq!(tone_icon(Tone::Neutral), Icon::INFO);
    assert_eq!(tone_icon(Tone::Accent), Icon::INFO);
    assert_eq!(tone_icon(Tone::Danger), Icon::CIRCLE_ALERT);
    assert_eq!(tone_icon(Tone::Warning), Icon::TRIANGLE_ALERT);
    assert_eq!(tone_icon(Tone::Success), Icon::CIRCLE_CHECK);
    let theme = Theme::dark();
    for tone in TONES {
        let mut h = H::new(Banner { tone, dismissed: 0 });
        let id = h.id("callout");
        let paint = h.paint(id);
        assert_eq!(paint.background, Some(theme.color(tone.background())));
        let mut edge = theme.color(tone.foreground());
        edge[3] *= 0.5;
        assert_eq!(paint.border_color, Some(edge));
        assert_eq!(paint.radii, [theme.sizes().radius; 4]);
        let info = h.ui.element(id).unwrap();
        assert_eq!(info.bounds.width, 800. - 20.);
        let node = h.ui.semantic_node(id).unwrap();
        let role = if matches!(tone, Tone::Danger | Tone::Warning) {
            SemanticRole::Alert
        } else {
            SemanticRole::Status
        };
        assert_eq!(node.role, role);
        // The icon is drawn in the tone's foreground.
        let icon =
            h.ui.elements()
                .find(|e| e.parent == Some(id) && e.kind == ElementType::Custom)
                .unwrap();
        assert_eq!(icon.color, theme.color(tone.foreground()));
        let title = h.id("title");
        assert_eq!(h.text(title), "Out of date");
        assert_eq!(
            h.ui.element(h.id("description")).unwrap().color,
            theme.palette().text_muted
        );
        // The action sits before the close button at the end.
        let action = h.ui.element(h.id("update")).unwrap().bounds;
        let close = h.ui.element(h.id("dismiss")).unwrap().bounds;
        assert!(action.x + action.width <= close.x);
        h.click(h.id("dismiss"));
        assert_eq!(h.read(|s| s.dismissed), 1);
        assert_eq!(
            h.ui.semantic_node(h.id("dismiss")).unwrap().label,
            Some("Dismiss")
        );
    }
}

struct App {
    toasts: Vec<Toast>,
    dismissed: Vec<u64>,
    actions: Vec<u64>,
    position: ToastPosition,
    behind: u32,
}
impl App {
    fn new(toasts: Vec<Toast>) -> Self {
        Self {
            toasts,
            dismissed: Vec::new(),
            actions: Vec::new(),
            position: ToastPosition::default(),
            behind: 0,
        }
    }
}
impl View for App {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        stack()
            .fill_width()
            .fill_height()
            .child(
                text_input("")
                    .key("editor")
                    .accessibility_label("Editor")
                    .on_change(cx.listener(|_, _: &TextChangeEvent, _| {})),
            )
            // Fills the window, so a click outside the toasts lands on it.
            .child(
                button("Behind")
                    .key("behind")
                    .absolute()
                    .left(0.)
                    .top(100.)
                    .right(0.)
                    .bottom(0.)
                    .on_click(cx.listener(|s, _: &ClickEvent, _| s.behind += 1)),
            )
            .child(
                toasts(cx, &self.toasts)
                    .position(self.position)
                    .on_dismiss(cx.listener(|s, e: &ToastDismiss, _| {
                        s.dismissed.push(e.id);
                        s.toasts.retain(|t| t.id != e.id);
                    }))
                    .on_action(cx.listener(|s, e: &ToastAction, _| s.actions.push(e.id))),
            )
    }
}

fn card(h: &H<App>, id: u64) -> ElementId {
    h.within(id, "toast")
}

/// Mounts `toasts` on a running frame clock with the editor focused.
fn running(toasts: Vec<Toast>) -> H<App> {
    let mut h = H::new(App::new(Vec::new()));
    h.r.set_reduce_motion(true);
    h.frame(1_000);
    let editor = h.id("editor");
    h.ui.focus(editor);
    h.prepare();
    h.update(|s| s.toasts = toasts);
    h
}

#[test]
fn toasts_expire_through_view_deadlines_from_their_first_fresh_frame() {
    let mut h = running(vec![
        Toast::new(1, "Saved").timeout(Some(Duration::from_secs(2))),
        Toast::new(2, "Pinned").timeout(None),
    ]);
    // The clock starts on the next fresh frame.
    assert!(h.r.animation_frame_requested());
    h.frame(1_016);
    assert_eq!(h.r.next_wakeup(), Some(Duration::from_millis(3_016)));
    let evaluations = h.ui.stats().component_evaluations;
    for t in [1_500, 2_000, 3_000] {
        assert!(!h.r.begin_frame(Duration::from_millis(t)));
        h.prepare();
    }
    assert_eq!(h.ui.stats().component_evaluations, evaluations);
    h.frame(3_016);
    assert_eq!(h.read(|s| s.dismissed.clone()), [1]);
    assert!(h.find(1u64).is_none());
    assert!(h.find(2u64).is_some());
    assert_eq!(h.r.next_wakeup(), None);
}

#[test]
fn hover_and_focus_pause_the_timeout() {
    let mut h = running(vec![
        Toast::new(1, "Saved").timeout(Some(Duration::from_secs(2))),
    ]);
    h.frame(1_016);
    // Hover at 2 s, after 984 ms shown.
    h.frame(2_000);
    let at = h.center(card(&h, 1));
    h.pointer(PointerEvent::Moved(at));
    assert_eq!(h.r.next_wakeup(), None);
    h.frame(5_000);
    assert!(h.read(|s| s.dismissed.is_empty()));
    // Leaving resumes on the next fresh frame with 1016 ms left.
    h.pointer(PointerEvent::Moved([10., 300.]));
    h.frame(6_000);
    assert_eq!(h.r.next_wakeup(), Some(Duration::from_millis(7_016)));

    // Focus in the toast pauses it too.
    let close = h.within(1u64, "dismiss");
    h.ui.focus(close);
    h.prepare();
    assert_eq!(h.r.next_wakeup(), None);
    h.frame(9_000);
    assert!(h.read(|s| s.dismissed.is_empty()));
    let editor = h.id("editor");
    h.ui.focus(editor);
    h.prepare();
    h.frame(9_100);
    // Focus came at 6 s, before the run began: 1016 ms are still left.
    assert_eq!(h.r.next_wakeup(), Some(Duration::from_millis(10_116)));
    h.frame(10_000);
    assert!(h.read(|s| s.dismissed.is_empty()));
    h.frame(10_116);
    assert_eq!(h.read(|s| s.dismissed.clone()), [1]);
}

#[test]
fn toasts_stack_in_their_corner_without_taking_focus_or_input() {
    let mut h = running(vec![
        Toast::new(1, "Older").timeout(None),
        Toast::new(2, "Newer")
            .tone(Tone::Danger)
            .description("Build failed.")
            .action("Retry")
            .timeout(None),
    ]);
    let editor = h.id("editor");
    assert_eq!(h.ui.focused_element(), Some(editor));
    let (older, newer) = (card(&h, 1), card(&h, 2));
    let older_box = h.ui.element(older).unwrap().bounds;
    let newer_box = h.ui.element(newer).unwrap().bounds;
    // The newest sits in the corner; earlier toasts stack away from it.
    assert_eq!(newer_box.width, 360.);
    assert_eq!(newer_box.x, 800. - 16. - 360.);
    assert_eq!(newer_box.y + newer_box.height, 600. - 16.);
    assert_eq!(older_box.y + older_box.height, newer_box.y - 8.);
    let node = h.ui.semantic_node(newer).unwrap();
    assert_eq!(node.role, SemanticRole::Alert);
    assert_eq!(
        h.ui.element(newer).unwrap().paint.radii[0],
        Theme::dark().sizes().radius_large
    );

    // The window behind the stack keeps its input outside the cards.
    h.click(h.id("behind"));
    assert_eq!(h.read(|s| s.behind), 1);
    let below = [newer_box.x + 10., newer_box.y - 4.];
    h.pointer(PointerEvent::Moved(below));
    h.pointer(PointerEvent::Pressed(below));
    h.pointer(PointerEvent::Released(below));
    assert_eq!(h.read(|s| s.behind), 2);
    assert_eq!(h.read(|s| s.toasts.len()), 2);

    // Action and close buttons propose through their listeners.
    h.click(h.within(2u64, "action"));
    assert_eq!(h.read(|s| s.actions.clone()), [2]);
    h.click(h.within(1u64, "dismiss"));
    assert_eq!(h.read(|s| s.dismissed.clone()), [1]);
    let moved = h.ui.element(card(&h, 2)).unwrap().bounds;
    assert_eq!(moved.y + moved.height, 600. - 16.);

    h.update(|s| s.position = ToastPosition::TopStart);
    let top = h.ui.element(card(&h, 2)).unwrap().bounds;
    assert_eq!([top.x, top.y], [16., 16.]);
}

#[test]
fn a_polite_live_region_announces_each_new_toast_once() {
    let mut h = running(Vec::new());
    let region = h.id("announcement");
    let node = h.ui.semantic_node(region).unwrap();
    assert_eq!(node.live, Some(LivePoliteness::Polite));
    assert_eq!(node.value, Some(""));
    h.update(|s| {
        s.toasts
            .push(Toast::new(1, "Saved").description("All files."))
    });
    let region = h.id("announcement");
    assert_eq!(
        h.ui.semantic_node(region).unwrap().value,
        Some("Saved. All files.")
    );
    h.update(|s| s.toasts.push(Toast::new(2, "Pushed")));
    let region = h.id("announcement");
    assert_eq!(h.ui.semantic_node(region).unwrap().value, Some("Pushed"));
    // Removing the newest does not announce the older one again.
    h.update(|s| {
        s.toasts.pop();
    });
    let region = h.id("announcement");
    assert_eq!(h.ui.semantic_node(region).unwrap().value, Some("Pushed"));
    // The region is not drawn.
    let info = h.ui.element(h.id("announcer")).unwrap();
    assert_eq!([info.bounds.width, info.bounds.height], [0., 0.]);
}
