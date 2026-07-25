//! Reconciled adapters for specialized retained media and render workloads.

use std::sync::Arc;

use astrelis_core::geometry::LogicalSize;
pub use astrelis_paint::{CompositorViewId, ExternalImage, Image, ImageSampling};
pub use astrelis_ui_next::{ImageAlignment, ImageFit, RenderViewContent};
use astrelis_ui_next::{ImageElement, RenderView, UiInput};

use rxui_core::{ActionEmitter, RetainedSpec, View, retained};

/// Controlled raster-image presentation.
#[derive(Clone)]
pub struct ImageSpec {
    /// Immutable source image.
    pub image: Image,
    /// Accessible label.
    pub label: String,
    /// Preferred logical size.
    pub size: LogicalSize,
    /// Fitting policy.
    pub fit: ImageFit,
    /// Normalized alignment.
    pub alignment: ImageAlignment,
    /// Sampling policy.
    pub sampling: ImageSampling,
    /// Draw opacity.
    pub opacity: f32,
}

impl ImageSpec {
    /// Creates a centered contained image at source size.
    pub fn new(image: Image, label: impl Into<String>) -> Self {
        let element = ImageElement::new(image.clone(), label);
        Self {
            image,
            label: element.label,
            size: element.size,
            fit: element.fit,
            alignment: element.alignment,
            sampling: element.sampling,
            opacity: element.opacity,
        }
    }

    /// Selects a preferred logical size.
    pub const fn size(mut self, size: LogicalSize) -> Self {
        self.size = size;
        self
    }

    /// Selects image fitting.
    pub const fn fit(mut self, fit: ImageFit) -> Self {
        self.fit = fit;
        self
    }
}

impl<Action: 'static> RetainedSpec<Action> for ImageSpec {
    type Element = ImageElement;

    fn create(&self, _emitter: &ActionEmitter<Action>, _theme: &rxui_core::Theme) -> Self::Element {
        ImageElement {
            image: self.image.clone(),
            label: self.label.clone(),
            size: self.size,
            fit: self.fit,
            alignment: self.alignment,
            sampling: self.sampling,
            opacity: self.opacity,
        }
    }

    fn update(
        &self,
        element: &mut Self::Element,
        _emitter: &ActionEmitter<Action>,
        _theme: &rxui_core::Theme,
    ) {
        element.image = self.image.clone();
        element.label.clone_from(&self.label);
        element.size = self.size;
        element.fit = self.fit;
        element.alignment = self.alignment;
        element.sampling = self.sampling;
        element.opacity = self.opacity;
    }

    /// Reports the passes each field actually feeds.
    ///
    /// Three groups. `size` is the only layout input: `ImageElement::layout`
    /// constrains it and reads nothing else, and in particular does *not* derive
    /// a size from the source image, so a taller image alone never re-measures.
    /// `label` is only ever announced. Everything else, the source included, is
    /// consumed by `paint`.
    ///
    /// The source belongs to the announcement too, because the element reports
    /// the image's pixel dimensions as its accessible value. Keying that on the
    /// cache id over-reports - a same-sized replacement re-announces an
    /// unchanged string - which is the safe direction, and cheaper than
    /// comparing two [`Image`] headers to find out.
    fn changed(&self, previous: &Self) -> astrelis_ui_next::Invalidation {
        use astrelis_ui_next::Invalidation;

        let source_changed = self.image.cache_id() != previous.image.cache_id();
        let mut invalidation = Invalidation::empty();
        if self.size != previous.size {
            invalidation |= Invalidation::LAYOUT;
        }
        if source_changed || self.label != previous.label {
            invalidation |= Invalidation::ACCESSIBILITY;
        }
        if source_changed
            || self.fit != previous.fit
            || self.alignment != previous.alignment
            || self.sampling != previous.sampling
            || self.opacity != previous.opacity
        {
            invalidation |= Invalidation::PAINT;
        }
        invalidation
    }
}

/// Builds a retained raster image.
pub fn image<Action: 'static>(spec: ImageSpec) -> View<Action> {
    retained(spec)
}

/// Configuration for an application-rendered viewport.
pub struct RenderViewSpec<Action: 'static> {
    /// Accessible label.
    pub label: String,
    /// Preferred logical size.
    pub size: LogicalSize,
    /// Current texture or compositor content.
    pub content: RenderViewContent,
    on_input: Arc<dyn Fn(UiInput) -> Action>,
}

impl<Action: 'static> Clone for RenderViewSpec<Action> {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            size: self.size,
            content: self.content.clone(),
            on_input: self.on_input.clone(),
        }
    }
}

impl<Action: 'static> RenderViewSpec<Action> {
    /// Creates an interactive render viewport.
    pub fn new(
        label: impl Into<String>,
        size: LogicalSize,
        content: RenderViewContent,
        on_input: impl Fn(UiInput) -> Action + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            size,
            content,
            on_input: Arc::new(on_input),
        }
    }
}

impl<Action: 'static> RetainedSpec<Action> for RenderViewSpec<Action> {
    type Element = RenderView;

    fn create(&self, emitter: &ActionEmitter<Action>, _theme: &rxui_core::Theme) -> Self::Element {
        let mut element = RenderView::new(self.label.clone(), self.size);
        element.content = self.content.clone();
        let on_input = self.on_input.clone();
        let emitter = emitter.clone();
        element.set_input(move |input| emitter.emit(on_input(input)));
        element
    }

    fn update(
        &self,
        element: &mut Self::Element,
        emitter: &ActionEmitter<Action>,
        _theme: &rxui_core::Theme,
    ) {
        element.label.clone_from(&self.label);
        element.size = self.size;
        element.content = self.content.clone();
        let on_input = self.on_input.clone();
        let emitter = emitter.clone();
        element.set_input(move |input| emitter.emit(on_input(input)));
    }

    /// Reports the passes each field actually feeds.
    ///
    /// `size` is the layout input, `label` is announced and never drawn, and the
    /// content is both drawn and announced: `RenderView::accessibility` reports
    /// a failed frame's message as its accessible value, so a viewport that
    /// dropped its device has to say so and not merely turn red. The two silent
    /// content variants re-announce the same absent value when swapped for each
    /// other, which is a wasted accessibility visit rather than a wrong one, and
    /// cheaper than mirroring the engine's private mapping here so it can drift.
    ///
    /// Hit testing never changes: `RenderView::hit_testable` follows its input
    /// callback, and [`Self::update`] reinstalls that on every pass.
    fn changed(&self, previous: &Self) -> astrelis_ui_next::Invalidation {
        use astrelis_ui_next::Invalidation;

        let content_changed = self.content != previous.content;
        let mut invalidation = Invalidation::empty();
        if self.size != previous.size {
            invalidation |= Invalidation::LAYOUT;
        }
        if content_changed || self.label != previous.label {
            invalidation |= Invalidation::ACCESSIBILITY;
        }
        if content_changed {
            invalidation |= Invalidation::PAINT;
        }
        invalidation
    }
}

/// Builds an interactive retained render viewport.
pub fn render_surface<Action: 'static>(spec: RenderViewSpec<Action>) -> View<Action> {
    retained(spec)
}
