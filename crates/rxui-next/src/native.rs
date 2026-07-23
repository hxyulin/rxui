//! Native component window integration.

use astrelis_app::{App, AppContext};
#[cfg(not(target_arch = "wasm32"))]
use astrelis_app::{Runtime, RuntimeConfig, RuntimeError};
use astrelis_compositor::{CompositionStats, ViewOptions, ViewRenderTarget};
use astrelis_core::geometry::LogicalSize;
use astrelis_paint::CompositorViewId;
use astrelis_paint_gpu::ExternalImage;
use astrelis_paint_gpu::RenderStats;
#[cfg(not(target_arch = "wasm32"))]
use astrelis_platform::WindowId;
use astrelis_platform::{Window, WindowEvent};
pub use astrelis_ui_host::{GraphicsContext, WindowHostOptions};
use astrelis_ui_host::{HostError, HostUpdate, NextWindowHost};
use astrelis_ui_next::{Flex, FrameUpdate, UiError, UiRoot};

use crate::{Component, ComponentRuntime, ComponentServiceRequest, Theme};

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

    /// Drains clipboard and background-work requests for application coordination.
    pub fn drain_service_requests(&mut self) -> impl Iterator<Item = ComponentServiceRequest> + '_ {
        self.runtime.drain_service_requests()
    }

    /// Routes a completed host-service action back to its owning component.
    pub fn complete_service(
        &mut self,
        action: crate::ServiceAction,
    ) -> Result<FrameUpdate<'_>, UiError> {
        self.runtime.dispatch_erased(self.host.ui_mut(), action)
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

/// Minimal native application wrapper for a single effect-free component.
#[cfg(not(target_arch = "wasm32"))]
pub struct ComponentApplication<C: Component<Effect = ()>> {
    graphics: GraphicsContext,
    component: Option<C>,
    theme: Option<Theme>,
    options: Option<WindowHostOptions>,
    window: Option<ComponentWindow<C>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl<C: Component<Effect = ()>> ComponentApplication<C> {
    /// Creates a single-window component application.
    pub fn new(component: C, theme: Theme, options: WindowHostOptions) -> Self {
        Self {
            graphics: GraphicsContext::new(),
            component: Some(component),
            theme: Some(theme),
            options: Some(options),
            window: None,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<C: Component<Effect = ()>> App for ComponentApplication<C> {
    type Error = std::io::Error;

    fn resumed(&mut self, context: &mut AppContext<'_, '_, Self>) -> Result<(), Self::Error> {
        if self.window.is_none() {
            let component = self
                .component
                .take()
                .ok_or_else(|| std::io::Error::other("component was already mounted"))?;
            let theme = self
                .theme
                .take()
                .ok_or_else(|| std::io::Error::other("component theme is unavailable"))?;
            let options = self
                .options
                .take()
                .ok_or_else(|| std::io::Error::other("window options are unavailable"))?;
            let window = ComponentWindow::open(context, &self.graphics, component, theme, options)
                .map_err(std::io::Error::other)?;
            context.invalidate_window(window.window().id());
            self.window = Some(window);
        }
        Ok(())
    }

    fn window_event(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        id: WindowId,
        event: WindowEvent,
    ) -> Result<(), Self::Error> {
        let Some(window) = &mut self.window else {
            return Ok(());
        };
        if window.window().id() != id {
            return Ok(());
        }
        let update = window.handle_event(&event).map_err(std::io::Error::other)?;
        if update.close_requested {
            self.window = None;
            context.unregister_window(id);
            context.exit();
        } else if update.redraw {
            context.invalidate_window(id);
        }
        Ok(())
    }

    fn redraw(
        &mut self,
        _context: &mut AppContext<'_, '_, Self>,
        id: WindowId,
    ) -> Result<(), Self::Error> {
        if let Some(window) = &mut self.window
            && window.window().id() == id
        {
            window.redraw().map_err(std::io::Error::other)?;
        }
        Ok(())
    }
}

/// Runs one effect-free component in a native window.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_component<C: Component<Effect = ()>>(
    component: C,
    theme: Theme,
    options: WindowHostOptions,
) -> Result<(), RuntimeError<std::io::Error>> {
    Runtime::finish(astrelis_platform_winit::run_return(Runtime::new(
        ComponentApplication::new(component, theme, options),
        RuntimeConfig::default(),
    )))
    .map(|_| ())
}
