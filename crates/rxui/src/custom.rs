//! Application-defined leaf elements with their own measurement, painting and hit testing.
use std::any::Any;
#[cfg(feature = "rendering")]
use std::any::TypeId;

/// Leaf measurement input in logical units, excluding padding and border.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CustomMeasure {
    /// Width and height already fixed by styles or the parent, if any.
    pub known: [Option<f32>; 2],
    /// Space the parent offers along each axis.
    pub available: [taffy::AvailableSpace; 2],
    /// Inherited font size, for text-like content.
    pub font_size: f32,
}

/// An application-defined leaf element.
///
/// A custom element is an owned description, rebuilt by its view like any builder.
/// Reconciliation keeps the retained node while the type stays the same; a value
/// that compares unequal to the previous one is measured again. Wrap it with
/// [`custom`] and style, position, listen to and describe it with the ordinary
/// [`crate::Element`] builders, for example `.accessibility_role(...)`.
///
/// Painting needs the `rendering` feature: [`Self::prepare`] runs in
/// `UiPainter::prepare` with the Astrelis painter and fonts, outside any pass, and
/// [`Self::paint`] records draws in logical window units on a session that is
/// already scaled, offset and clipped for this element. `State` is retained per
/// placement by the painter and dropped with the element.
///
/// ```
/// # #[cfg(feature = "rendering")]
/// # {
/// use rxui::{CustomElement, CustomMeasure, ElementInfo, NumericValue, UiError, custom, prelude::*};
/// use rxui::astrelis::{PaintSession, Rect};
///
/// #[derive(PartialEq)]
/// struct Meter { fraction: f32 }
/// impl CustomElement for Meter {
///     type State = ();
///     fn measure(&self, request: CustomMeasure) -> [f32; 2] {
///         [request.known[0].unwrap_or(120.), 8.]
///     }
///     fn paint(&self, _: &(), element: &ElementInfo<'_>, paint: &mut PaintSession<'_, '_>)
///         -> Result<(), UiError> {
///         let b = element.content_bounds;
///         paint.fill_rect(Rect::new(b.x, b.y, b.width * self.fraction, b.height), element.color)?;
///         Ok(())
///     }
/// }
/// let meter = custom(Meter { fraction: 0.4 })
///     .accessibility_role(SemanticRole::ProgressIndicator)
///     .accessibility_label("Upload")
///     .accessibility_numeric_value(NumericValue { value: 0.4, min: 0., max: 1., step: None });
/// # }
/// ```
pub trait CustomElement: PartialEq + 'static {
    /// Painter-retained per-placement state, such as prepared text or paths.
    /// Use `()` when painting needs no retained resources.
    type State: Default + 'static;
    /// Content size for layout. The default is empty, for elements sized by styles.
    fn measure(&self, request: CustomMeasure) -> [f32; 2] {
        let _ = request;
        [0.; 2]
    }
    /// Whether `point`, relative to the border box of `size`, hits this element.
    /// Consulted only where the element already takes pointer input (listeners,
    /// a cursor, focus or `PointerEvents::Block`) and for wheel targeting.
    fn hit_test(&self, point: [f32; 2], size: [f32; 2]) -> bool {
        let _ = (point, size);
        true
    }
    /// Prepares GPU or text resources before the frame, outside painting.
    #[cfg(feature = "rendering")]
    fn prepare(
        &self,
        state: &mut Self::State,
        element: &crate::ElementInfo<'_>,
        cx: &mut CustomPrepare<'_>,
    ) -> Result<(), crate::UiError> {
        let _ = (state, element, cx);
        Ok(())
    }
    /// Records this element's draws after its background and border.
    #[cfg(feature = "rendering")]
    fn paint(
        &self,
        state: &Self::State,
        element: &crate::ElementInfo<'_>,
        paint: &mut astrelis::PaintSession<'_, '_>,
    ) -> Result<(), crate::UiError> {
        let _ = (state, element, paint);
        Ok(())
    }
}

/// Resources lent to [`CustomElement::prepare`].
#[cfg(feature = "rendering")]
pub struct CustomPrepare<'a> {
    /// Painter for text, path and image preparation.
    pub painter: &'a mut astrelis::Painter,
    /// Loaded fonts for shaping. Load new fonts through `UiPainter::fonts_mut` so
    /// measurement is invalidated.
    pub fonts: &'a mut astrelis::TextSystem,
    /// Destination format of the frame.
    pub format: &'a astrelis::RenderFormat,
    /// Physical pixels per logical unit.
    pub raster_scale: f32,
}

/// Wraps a custom element in an ordinary leaf [`crate::Element`].
pub fn custom(element: impl CustomElement) -> crate::Element {
    crate::Element::new(crate::element::ElementKind::Custom(std::rc::Rc::new(
        element,
    )))
}

/// Object-safe form of [`CustomElement`] stored in descriptions.
pub(crate) trait AnyCustom {
    fn as_any(&self) -> &dyn Any;
    fn same(&self, other: &dyn AnyCustom) -> bool;
    fn measure(&self, request: CustomMeasure) -> [f32; 2];
    fn hit_test(&self, point: [f32; 2], size: [f32; 2]) -> bool;
    #[cfg(feature = "rendering")]
    fn state_type(&self) -> TypeId;
    #[cfg(feature = "rendering")]
    fn new_state(&self) -> Box<dyn Any>;
    #[cfg(feature = "rendering")]
    fn prepare(
        &self,
        state: &mut dyn Any,
        element: &crate::ElementInfo<'_>,
        cx: &mut CustomPrepare<'_>,
    ) -> Result<(), crate::UiError>;
    #[cfg(feature = "rendering")]
    fn paint(
        &self,
        state: &dyn Any,
        element: &crate::ElementInfo<'_>,
        paint: &mut astrelis::PaintSession<'_, '_>,
    ) -> Result<(), crate::UiError>;
}
impl<E: CustomElement> AnyCustom for E {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn same(&self, other: &dyn AnyCustom) -> bool {
        other.as_any().downcast_ref::<E>() == Some(self)
    }
    fn measure(&self, request: CustomMeasure) -> [f32; 2] {
        CustomElement::measure(self, request)
    }
    fn hit_test(&self, point: [f32; 2], size: [f32; 2]) -> bool {
        CustomElement::hit_test(self, point, size)
    }
    #[cfg(feature = "rendering")]
    fn state_type(&self) -> TypeId {
        TypeId::of::<E::State>()
    }
    #[cfg(feature = "rendering")]
    fn new_state(&self) -> Box<dyn Any> {
        Box::new(E::State::default())
    }
    #[cfg(feature = "rendering")]
    fn prepare(
        &self,
        state: &mut dyn Any,
        element: &crate::ElementInfo<'_>,
        cx: &mut CustomPrepare<'_>,
    ) -> Result<(), crate::UiError> {
        let state = state
            .downcast_mut()
            .expect("custom state matches its element");
        CustomElement::prepare(self, state, element, cx)
    }
    #[cfg(feature = "rendering")]
    fn paint(
        &self,
        state: &dyn Any,
        element: &crate::ElementInfo<'_>,
        paint: &mut astrelis::PaintSession<'_, '_>,
    ) -> Result<(), crate::UiError> {
        let state = state
            .downcast_ref()
            .expect("custom state matches its element");
        CustomElement::paint(self, state, element, paint)
    }
}
