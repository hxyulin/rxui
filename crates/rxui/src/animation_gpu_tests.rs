use crate::*;
use astrelis::GraphicsContext;
use std::time::Duration;

struct Dialog {
    open: bool,
}
impl View for Dialog {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut page = column().size(64., 64.);
        if self.open {
            page = page.child(
                modal(
                    column()
                        .key("content")
                        .size(24., 24.)
                        .background([1., 0., 0., 1.])
                        .no_transition(),
                )
                .key("dialog")
                .padding(0.)
                .transition(Transition::linear(Duration::from_millis(100))),
            );
        }
        page
    }
}
/// RGBA8 at the dialog content's center, after preparing frame `time`.
fn center(test: &mut TestUi<Dialog>, time: u64) -> [u8; 4] {
    test.runtime().begin_frame(Duration::from_millis(time));
    let pixels = test.render().unwrap();
    let i = (32 * 64 + 32) * 4;
    pixels[i..i + 4].try_into().unwrap()
}
#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn overlay_fades_paint_through_a_layer_and_the_exit_visual_lingers() {
    let graphics = pollster::block_on(GraphicsContext::headless()).unwrap();
    let mut test = TestUi::new(&graphics, [64, 64], Dialog { open: false }).unwrap();
    assert_eq!(center(&mut test, 1000)[3], 0);
    let entity = test.entity().clone();
    test.runtime()
        .update(|cx| entity.update(cx, |d, _| d.open = true));
    // Entering: invisible on the first frame, half way after 50 ms, then opaque.
    assert_eq!(center(&mut test, 1016)[3], 0);
    let half = center(&mut test, 1066);
    assert!((120..=136).contains(&half[3]), "{half:?}");
    assert!(half[0] > 120 && half[1] < 8);
    assert_eq!(center(&mut test, 1116), [255, 0, 0, 255]);
    assert!(!test.ui().is_animating());
    // Exiting: the content is gone from the tree, but its pixels fade out.
    test.runtime()
        .update(|cx| entity.update(cx, |d, _| d.open = false));
    assert_eq!(center(&mut test, 1200), [255, 0, 0, 255]);
    assert!(
        test.ui()
            .semantics()
            .all(|n| n.role != SemanticRole::Dialog)
    );
    let half = center(&mut test, 1250);
    assert!((120..=136).contains(&half[3]), "{half:?}");
    assert_eq!(center(&mut test, 1300)[3], 0);
    assert!(!test.ui().is_animating() && !test.ui().needs_composition());
}
