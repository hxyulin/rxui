//! Native component window integration.

use astrelis_app::{App, AppContext};
use astrelis_compositor::{CompositionStats, ViewOptions, ViewRenderTarget};
use astrelis_core::geometry::LogicalSize;
use astrelis_paint::CompositorViewId;
use astrelis_paint_gpu::ExternalImage;
use astrelis_paint_gpu::RenderStats;
use astrelis_platform::{Window, WindowEvent};
use astrelis_ui_host::{GraphicsContext, HostError, HostUpdate, NextWindowHost, WindowHostOptions};
use astrelis_ui_next::{Flex, FrameUpdate, UiError, UiRoot};

use crate::{Component, ComponentRuntime, Theme};

/// State-owning component runtime connected to one native window.
pub struct ComponentWindow<C: Component> {
    host: NextWindowHost,
    runtime: ComponentRuntime<C>,
}

impl<C: Component> ComponentWindow<C> {
    /// Opens a native window and mounts the root component into it.
    pub fn open<A: App>(
        context: &mut AppContext<'_, '_, A>,
        graphics: &GraphicsContext,
        component: C,
        theme: Theme,
        options: WindowHostOptions,
    ) -> Result<Self, HostError> {
        let mut ui = UiRoot::new(Flex::default(), LogicalSize::new(1.0, 1.0));
        let runtime = ComponentRuntime::mount(component, &mut ui, theme)
            .map_err(|error| HostError::new(error.to_string()))?;
        let host = NextWindowHost::open(context, graphics, ui, options)?;
        Ok(Self { host, runtime })
    }

    /// Returns the platform window.
    pub const fn window(&self) -> &Window {
        self.host.window()
    }

    /// Returns the low-level incremental window host.
    pub const fn host(&self) -> &NextWindowHost {
        &self.host
    }

    /// Returns the low-level incremental window host for GPU integration.
    pub fn host_mut(&mut self) -> &mut NextWindowHost {
        &mut self.host
    }

    /// Reads root component state.
    pub const fn component(&self) -> &C {
        self.runtime.component()
    }

    /// Mutates root state before [`Self::refresh`].
    pub fn component_mut(&mut self) -> &mut C {
        self.runtime.component_mut()
    }

    /// Routes one platform event through the retained UI and component tree.
    pub fn handle_event(&mut self, event: &WindowEvent) -> Result<HostUpdate, HostError> {
        let mut update = self.host.handle_event(event)?;
        let actions = self.host.drain_actions().collect::<Vec<_>>();
        if !actions.is_empty() {
            for action in actions {
                self.runtime
                    .dispatch_erased(self.host.ui_mut(), action)
                    .map_err(|error| HostError::new(error.to_string()))?;
            }
            update.redraw = true;
        }
        Ok(update)
    }

    /// Applies one root action and reconciles the component tree.
    pub fn dispatch(&mut self, action: C::Action) -> Result<FrameUpdate<'_>, UiError> {
        self.runtime.dispatch(self.host.ui_mut(), action)
    }

    /// Reconciles after application-owned state mutation.
    pub fn refresh(&mut self) -> Result<FrameUpdate<'_>, UiError> {
        self.runtime.refresh(self.host.ui_mut())
    }

    /// Replaces typed theme tokens and reconciles resolved styles.
    pub fn set_theme(&mut self, theme: Theme) -> Result<FrameUpdate<'_>, UiError> {
        self.runtime.set_theme(self.host.ui_mut(), theme)
    }

    /// Drains effects emitted by the root or nested components.
    pub fn drain_effects(&mut self) -> impl Iterator<Item = C::Effect> + '_ {
        self.runtime.drain_effects()
    }

    /// Generates and presents one component UI frame.
    pub fn redraw(&mut self) -> Result<Option<RenderStats>, HostError> {
        self.host.redraw()
    }

    /// Registers an application-owned texture sampled by retained paint.
    pub fn register_external_image(
        &mut self,
        image: &ExternalImage,
        view: astrelis_gpu::TextureView,
    ) -> Result<(), HostError> {
        self.host.register_external_image(image, view)
    }

    /// Removes a previously registered retained-paint image.
    pub fn unregister_external_image(&mut self, image: &ExternalImage) -> bool {
        self.host.unregister_external_image(image)
    }

    /// Presents a frame with application-rendered compositor views.
    pub fn redraw_composited<E>(
        &mut self,
        view_options: impl FnMut(CompositorViewId) -> ViewOptions,
        render_view: impl FnMut(
            CompositorViewId,
            &mut astrelis_gpu::CommandEncoder,
            ViewRenderTarget,
        ) -> Result<(), E>,
    ) -> Result<Option<CompositionStats>, HostError>
    where
        E: std::fmt::Display,
    {
        self.host.redraw_composited(view_options, render_view)
    }
}
