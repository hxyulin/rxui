//! Headless placement harness: offscreen rendering, synthetic input and queries.
use crate::{
    ElementInfo, Entity, InputResult, Key, KeyEvent, Modifiers, PointerButton, PointerEvent,
    Runtime, SemanticAction, SemanticNode, TextInputEvent, TextMeasure, Ui, UiError, UiPainter,
    View,
};
use astrelis::{FramebufferOptions, GraphicsContext, wgpu};

/// One placement rendered offscreen, for tests and tools without a window.
///
/// It owns a [`Runtime`], the [`Ui`] for one root view and a [`UiPainter`], lays out
/// at a fixed logical size with a raster scale of one, and renders through
/// `Framebuffer::read_rgba8`. Input methods prepare stale geometry first, like the
/// native host, and route through the same `Ui` entry points with the painter as
/// text measurement. Effects keep their normal boundary: call
/// `runtime().flush()` where a test depends on them.
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use rxui::{TestUi, prelude::*, astrelis::GraphicsContext};
/// struct Counter(u32);
/// impl View for Counter {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         button(format!("Count {}", self.0))
///             .key("increment")
///             .on_click(cx.listener(|this, _, _| this.0 += 1))
///     }
/// }
/// let graphics = pollster::block_on(GraphicsContext::headless())?;
/// let mut test = TestUi::new(&graphics, [200, 80], Counter(0))?;
/// test.painter_mut().fonts_mut().load_system_fonts();
/// test.click("increment")?;
/// assert_eq!(test.read(|counter| counter.0), 1);
/// let pixels = test.render()?; // 200 x 80 RGBA8, row-major.
/// # Ok(()) }
/// ```
pub struct TestUi<T: View> {
    graphics: GraphicsContext,
    runtime: Runtime,
    ui: Ui<T>,
    painter: UiPainter,
    size: [u32; 2],
}
impl<T: View> TestUi<T> {
    /// Mounts `root` in a new runtime with a logical size of `size`.
    pub fn new(graphics: &GraphicsContext, size: [u32; 2], root: T) -> Result<Self, UiError> {
        let mut runtime = Runtime::new();
        let entity = runtime.update(|cx| cx.new(|_| root));
        let ui = Ui::new(&mut runtime, entity)?;
        Ok(Self {
            painter: UiPainter::new(graphics),
            graphics: graphics.clone(),
            runtime,
            ui,
            size,
        })
    }
    /// The runtime, for updates, flushes and entity reads.
    pub fn runtime(&mut self) -> &mut Runtime {
        &mut self.runtime
    }
    /// The placement.
    pub fn ui(&self) -> &Ui<T> {
        &self.ui
    }
    /// The painter, for loading fonts and inspecting resource statistics.
    pub fn painter_mut(&mut self) -> &mut UiPainter {
        &mut self.painter
    }
    /// All three parts at once, for `Ui` methods the harness does not wrap.
    pub fn parts(&mut self) -> (&mut Runtime, &mut Ui<T>, &mut UiPainter) {
        (&mut self.runtime, &mut self.ui, &mut self.painter)
    }
    /// The root entity.
    pub fn entity(&self) -> &Entity<T> {
        self.ui.entity()
    }
    /// Reads the root view's state.
    pub fn read<R>(&mut self, f: impl FnOnce(&T) -> R) -> R {
        let entity = self.ui.entity().clone();
        self.runtime.update(|cx| f(&entity.read(cx)))
    }
    /// Changes the logical size used by the next preparation.
    pub fn resize(&mut self, size: [u32; 2]) {
        self.size = size;
        self.ui.invalidate_geometry();
    }
    /// Reconciles and lays out if the views or fonts changed since the last preparation.
    pub fn prepare(&mut self) -> Result<(), UiError> {
        if !self.ui.is_prepared()
            || self.ui.needs_prepare(&self.runtime)?
            || self.ui.measurement_generation() != self.painter.generation()
        {
            let size = [self.size[0] as f32, self.size[1] as f32];
            self.ui
                .prepare(&mut self.runtime, size, &mut self.painter)?;
        }
        Ok(())
    }
    /// Prepares and renders one frame over transparent black, returning
    /// `width * height` RGBA8 pixels in row-major order.
    pub fn render(&mut self) -> Result<Vec<u8>, UiError> {
        self.prepare()?;
        render_rgba8(
            &self.graphics,
            &self.ui,
            &mut self.painter,
            self.size,
            wgpu::Color::TRANSPARENT,
        )
    }
    /// The first visible element with `key`, in paint order. Keys are unique among
    /// siblings only; use `ui().elements()` to tell nested duplicates apart.
    pub fn element(&mut self, key: impl Into<Key>) -> Result<Option<ElementInfo<'_>>, UiError> {
        self.prepare()?;
        let key = key.into();
        Ok(self.ui.elements().find(|e| e.key == Some(&key)))
    }
    /// Semantics of the first visible element with `key`.
    pub fn semantics(&mut self, key: impl Into<Key>) -> Result<Option<SemanticNode<'_>>, UiError> {
        let Some(id) = self.element(key)?.map(|e| e.id) else {
            return Ok(None);
        };
        Ok(self.ui.semantic_node(id))
    }
    /// Presses and releases the primary button at the center of the element with `key`.
    ///
    /// # Panics
    ///
    /// If no visible element has `key`.
    pub fn click(&mut self, key: impl Into<Key>) -> Result<bool, UiError> {
        let key = key.into();
        let b = self
            .element(key.clone())?
            .unwrap_or_else(|| panic!("no visible element keyed {key:?}"))
            .bounds;
        let position = [b.x + b.width / 2., b.y + b.height / 2.];
        let modifiers = Modifiers::default();
        let button = PointerButton::Primary;
        let down = self.pointer(PointerEvent::Down {
            position,
            button,
            modifiers,
        })?;
        let up = self.pointer(PointerEvent::Up {
            position,
            button,
            modifiers,
        })?;
        Ok(down | up)
    }
    /// Routes a pointer event with text caret placement, as the native host does.
    pub fn pointer(&mut self, event: PointerEvent) -> Result<bool, UiError> {
        self.prepare()?;
        self.ui
            .pointer_with_text(&mut self.runtime, event, &mut self.painter, false)
    }
    /// Routes a wheel event at a logical position.
    pub fn wheel(&mut self, position: [f32; 2], delta: [f32; 2]) -> Result<InputResult, UiError> {
        self.prepare()?;
        self.ui
            .wheel(&mut self.runtime, position, delta, Modifiers::default())
    }
    /// Routes a key event to the focused element's listeners. Focus traversal and
    /// text editing keys are separate: use [`Self::text_input`] and `Ui::focus_next`.
    pub fn key(&mut self, event: KeyEvent) -> Result<InputResult, UiError> {
        self.prepare()?;
        self.ui.key(&mut self.runtime, event)
    }
    /// Applies a text editing event to the focused text input.
    pub fn text_input(&mut self, event: TextInputEvent) -> Result<bool, UiError> {
        self.prepare()?;
        self.ui
            .text_input(&mut self.runtime, event, &mut self.painter)
    }
    /// Applies an assistive technology action.
    pub fn semantic_action(&mut self, action: SemanticAction) -> Result<bool, UiError> {
        self.prepare()?;
        self.ui
            .semantic_action(&mut self.runtime, action, &mut self.painter)
    }
}

/// Paints a prepared placement into a new linear RGBA8 framebuffer at raster scale one.
pub(crate) fn render_rgba8<T: View>(
    graphics: &GraphicsContext,
    ui: &Ui<T>,
    painter: &mut UiPainter,
    size: [u32; 2],
    clear: wgpu::Color,
) -> Result<Vec<u8>, UiError> {
    let mut target = graphics.create_framebuffer(
        FramebufferOptions::new(size[0], size[1])
            .format(wgpu::TextureFormat::Rgba8Unorm)
            .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
    )?;
    painter.prepare(ui, &target.render_format(), 1.)?;
    // An offscreen target only refuses a frame when it is empty.
    let mut frame = target.begin_frame().map_err(|_| UiError::InvalidGeometry)?;
    painter.compose(ui, &mut frame, 1., |frame, composed| {
        let mut pass = frame.render_pass().clear_color(clear).begin()?;
        composed.paint(&mut pass)
    })?;
    frame.finish()?;
    Ok(target.read_rgba8()?)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{IntoElement, SemanticRole, TextChangeEvent, ViewContext, button, column};

    pub(crate) struct Form {
        pub(crate) count: u32,
        pub(crate) name: String,
    }
    impl View for Form {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            column()
                .child(
                    column()
                        .key("swatch")
                        .size(20., 20.)
                        .background([1., 0., 0., 1.]),
                )
                .child(
                    button(format!("Count {}", self.count))
                        .key("increment")
                        .on_click(cx.listener(|this, _, _| this.count += 1)),
                )
                .child(
                    crate::text_input(self.name.clone())
                        .key("name")
                        .width(120.)
                        .on_change(cx.listener(|this, edit: &TextChangeEvent, _| {
                            this.name = edit.value.clone()
                        })),
                )
        }
    }
    pub(crate) fn form(graphics: &GraphicsContext) -> TestUi<Form> {
        let mut test = TestUi::new(
            graphics,
            [160, 120],
            Form {
                count: 0,
                name: String::new(),
            },
        )
        .unwrap();
        test.painter_mut()
            .fonts_mut()
            .load_font(include_bytes!("../tests/fonts/SourceSans3-Regular.otf"))
            .unwrap();
        test
    }

    #[test]
    #[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
    fn harness_renders_routes_input_and_answers_queries() {
        let graphics = pollster::block_on(GraphicsContext::headless()).unwrap();
        let mut test = form(&graphics);
        let swatch = test.element("swatch").unwrap().unwrap().bounds;
        assert_eq!([swatch.width, swatch.height], [20., 20.]);
        assert!(test.element("missing").unwrap().is_none());
        assert_eq!(
            test.semantics("increment").unwrap().unwrap().role,
            SemanticRole::Button
        );

        assert!(test.click("increment").unwrap());
        assert!(test.click("increment").unwrap());
        assert_eq!(test.read(|form| form.count), 2);
        let label = test.semantics("increment").unwrap().unwrap().label;
        assert_eq!(label, Some("Count 2"));

        test.click("name").unwrap();
        test.text_input(TextInputEvent::Insert("Ada".into()))
            .unwrap();
        assert_eq!(test.read(|form| form.name.clone()), "Ada");

        let pixels = test.render().unwrap();
        assert_eq!(pixels.len(), 160 * 120 * 4);
        assert_eq!(&pixels[..4], &[255, 0, 0, 255]);
        assert_eq!(&pixels[(119 * 160 + 159) * 4..][..4], &[0, 0, 0, 0]);
        test.resize([0, 0]);
        assert!(matches!(test.render(), Err(UiError::InvalidGeometry)));
    }
}
