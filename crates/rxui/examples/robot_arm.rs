//! Native and WebGPU five-degree-of-freedom robotic-arm editor.
//!
//! Runs on the high-level [`rxui::app`] runner. The `render` hook composites
//! the 3D scene through [`AppCx::host`] and `WindowHost::redraw_composited`;
//! on the web the same application starts through
//! [`rxui::app::spawn_on_canvas`] on the host page canvas.

#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use astrelis_compositor::{CompositionStats, ViewOptions, ViewRenderTarget};
use astrelis_core::math::{EulerRot, Mat4, Vec3};
use astrelis_paint::CompositorViewId;
use astrelis_platform::{ElementState, PointerButton};
use astrelis_render::RenderStats;
use astrelis_render_3d::{
    Camera3D, DrawList3D, Lighting, MaterialDescriptor, MaterialHandle, MeshDraw, MeshHandle,
    Renderer3D, cube, plane, uv_sphere,
};
use rxui::{
    editor::widgets::{
        RenderView, RenderViewContent, RenderViewEvent, SplitAxis, SplitPane, SplitPaneOptions,
    },
    prelude::*,
};

const JOINTS: [JointSpec; 5] = [
    JointSpec::new("Base yaw", "Y", -180.0, 180.0),
    JointSpec::new("Shoulder pitch", "Z", -90.0, 90.0),
    JointSpec::new("Elbow pitch", "Z", -135.0, 135.0),
    JointSpec::new("Wrist pitch", "Z", -120.0, 120.0),
    JointSpec::new("Wrist roll", "Y", -180.0, 180.0),
];
const ZERO_POSE: JointPose = JointPose::new([0.0; 5]);
const HOME_POSE: JointPose = JointPose::new([20.0, -35.0, 70.0, -25.0, 15.0]);
const LINKS: LinkDimensions = LinkDimensions {
    base_height: 0.55,
    upper_arm: 1.8,
    forearm: 1.55,
    wrist: 0.65,
    tool: 0.38,
};

#[derive(Clone, Copy, Debug)]
struct JointSpec {
    name: &'static str,
    axis: &'static str,
    min: f32,
    max: f32,
}

impl JointSpec {
    const fn new(name: &'static str, axis: &'static str, min: f32, max: f32) -> Self {
        Self {
            name,
            axis,
            min,
            max,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct JointPose {
    degrees: [f32; 5],
}

impl JointPose {
    const fn new(degrees: [f32; 5]) -> Self {
        Self { degrees }
    }

    fn set(&mut self, joint: usize, value: f32) {
        let spec = JOINTS[joint];
        self.degrees[joint] = value.round().clamp(spec.min, spec.max);
    }

    fn clamped(mut self) -> Self {
        for joint in 0..self.degrees.len() {
            self.set(joint, self.degrees[joint]);
        }
        self
    }
}

#[derive(Clone, Copy, Debug)]
struct LinkDimensions {
    base_height: f32,
    upper_arm: f32,
    forearm: f32,
    wrist: f32,
    tool: f32,
}

#[derive(Clone, Copy, Debug)]
struct ArmTransforms {
    base: Mat4,
    shoulder: Mat4,
    elbow: Mat4,
    wrist_pitch: Mat4,
    wrist_roll: Mat4,
    end_effector: Mat4,
}

fn forward_kinematics(pose: JointPose) -> ArmTransforms {
    let pose = pose.clamped();
    let radians = pose.degrees.map(f32::to_radians);
    let base = Mat4::from_rotation_y(radians[0]);
    let shoulder = base
        * Mat4::from_translation(Vec3::Y * LINKS.base_height)
        * Mat4::from_rotation_z(radians[1]);
    let elbow = shoulder
        * Mat4::from_translation(Vec3::Y * LINKS.upper_arm)
        * Mat4::from_rotation_z(radians[2]);
    let wrist_pitch =
        elbow * Mat4::from_translation(Vec3::Y * LINKS.forearm) * Mat4::from_rotation_z(radians[3]);
    let wrist_roll = wrist_pitch
        * Mat4::from_translation(Vec3::Y * LINKS.wrist)
        * Mat4::from_rotation_y(radians[4]);
    let end_effector = wrist_roll * Mat4::from_translation(Vec3::Y * LINKS.tool);
    ArmTransforms {
        base,
        shoulder,
        elbow,
        wrist_pitch,
        wrist_roll,
        end_effector,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct EndEffectorPose {
    position: Vec3,
    yaw_pitch_roll: Vec3,
}

fn end_effector_pose(pose: JointPose) -> EndEffectorPose {
    let matrix = forward_kinematics(pose).end_effector;
    let (_, rotation, position) = matrix.to_scale_rotation_translation();
    let (yaw, pitch, roll) = rotation.to_euler(EulerRot::YXZ);
    EndEffectorPose {
        position,
        yaw_pitch_roll: Vec3::new(yaw.to_degrees(), pitch.to_degrees(), roll.to_degrees()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct OrbitCamera {
    azimuth: f32,
    elevation: f32,
    distance: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            azimuth: 35.0,
            elevation: 25.0,
            distance: 8.5,
        }
    }
}

impl OrbitCamera {
    fn orbit(&mut self, dx: f32, dy: f32) {
        self.azimuth = (self.azimuth - dx * 0.35).rem_euclid(360.0);
        self.elevation = (self.elevation + dy * 0.35).clamp(-10.0, 85.0);
    }

    fn zoom(&mut self, delta: f32) {
        self.distance = (self.distance * (-delta * 0.0015).exp()).clamp(3.0, 16.0);
    }

    fn camera(self) -> Camera3D {
        let azimuth = self.azimuth.to_radians();
        let elevation = self.elevation.to_radians();
        let target = Vec3::new(0.0, 1.6, 0.0);
        let offset = Vec3::new(
            elevation.cos() * azimuth.sin(),
            elevation.sin(),
            elevation.cos() * azimuth.cos(),
        ) * self.distance;
        let mut camera = Camera3D {
            position: target + offset,
            ..Default::default()
        };
        camera.look_at(target, Vec3::Y);
        camera
    }
}

#[derive(Clone, Debug)]
enum Message {
    Joint(usize, f32),
    Home,
    Zero,
    ResetCamera,
    View(RenderViewEvent),
}

struct UiHandles {
    sliders: [ElementHandle<Slider>; 5],
    values: [ElementHandle<Label>; 5],
    position: ElementHandle<Label>,
    orientation: ElementHandle<Label>,
    selected: ElementHandle<Label>,
    frame_stats: ElementHandle<Label>,
}

struct SceneGpu {
    renderer: Renderer3D,
    cube: MeshHandle,
    sphere: MeshHandle,
    plane: MeshHandle,
    materials: [MaterialHandle; 7],
}

impl SceneGpu {
    fn new(device: astrelis_gpu::Device, queue: astrelis_gpu::Queue) -> rxui::Result<Self> {
        let mut renderer = Renderer3D::new(device, queue, Default::default())?;
        let cube = renderer.create_mesh(&cube(1.0))?;
        let sphere = renderer.create_mesh(&uv_sphere(1.0, 24, 12))?;
        let plane = renderer.create_mesh(&plane(12.0, 12.0))?;
        let colors = [
            Color::rgb(0.88, 0.22, 0.18),
            Color::rgb(0.95, 0.54, 0.12),
            Color::rgb(0.92, 0.82, 0.18),
            Color::rgb(0.18, 0.72, 0.42),
            Color::rgb(0.16, 0.52, 0.9),
            Color::rgb(0.62, 0.3, 0.88),
            Color::rgb(0.09, 0.12, 0.18),
        ];
        let materials = colors
            .map(|base_color| {
                renderer
                    .create_material(MaterialDescriptor {
                        base_color,
                        double_sided: true,
                        ..Default::default()
                    })
                    .map_err(rxui::Error::from)
            })
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?
            .try_into()
            .expect("material count is fixed");
        Ok(Self {
            renderer,
            cube,
            sphere,
            plane,
            materials,
        })
    }

    fn draw_list(&self, pose: JointPose) -> DrawList3D {
        let transforms = forward_kinematics(pose);
        let mut list = DrawList3D::new();
        list.draw_mesh(MeshDraw {
            mesh: self.plane,
            material: self.materials[6],
            transform: Mat4::from_translation(Vec3::new(0.0, -0.035, 0.0)),
            tint: Color::WHITE,
        });
        list.draw_grid(12, 0.35, Color::new(0.25, 0.33, 0.48, 0.62));
        list.draw_axes(Mat4::IDENTITY, 0.9);

        let link = |list: &mut DrawList3D,
                    transform: Mat4,
                    length: f32,
                    thickness: f32,
                    material: MaterialHandle| {
            list.draw_mesh(MeshDraw {
                mesh: self.cube,
                material,
                transform: transform
                    * Mat4::from_translation(Vec3::Y * length * 0.5)
                    * Mat4::from_scale(Vec3::new(thickness, length, thickness)),
                tint: Color::WHITE,
            });
        };
        let joint = |list: &mut DrawList3D, transform: Mat4, material: MaterialHandle| {
            list.draw_mesh(MeshDraw {
                mesh: self.sphere,
                material,
                transform: transform * Mat4::from_scale(Vec3::splat(0.18)),
                tint: Color::WHITE,
            });
        };

        link(
            &mut list,
            transforms.base,
            LINKS.base_height,
            0.48,
            self.materials[0],
        );
        joint(&mut list, transforms.shoulder, self.materials[1]);
        link(
            &mut list,
            transforms.shoulder,
            LINKS.upper_arm,
            0.27,
            self.materials[1],
        );
        joint(&mut list, transforms.elbow, self.materials[2]);
        link(
            &mut list,
            transforms.elbow,
            LINKS.forearm,
            0.23,
            self.materials[2],
        );
        joint(&mut list, transforms.wrist_pitch, self.materials[3]);
        link(
            &mut list,
            transforms.wrist_pitch,
            LINKS.wrist,
            0.18,
            self.materials[3],
        );
        joint(&mut list, transforms.wrist_roll, self.materials[4]);
        link(
            &mut list,
            transforms.wrist_roll,
            LINKS.tool,
            0.14,
            self.materials[4],
        );

        let gripper = transforms.end_effector;
        for x in [-0.16, 0.16] {
            list.draw_mesh(MeshDraw {
                mesh: self.cube,
                material: self.materials[5],
                transform: gripper
                    * Mat4::from_translation(Vec3::new(x, 0.13, 0.0))
                    * Mat4::from_scale(Vec3::new(0.09, 0.32, 0.12)),
                tint: Color::WHITE,
            });
        }
        list.draw_axes(transforms.end_effector, 0.45);
        list
    }

    fn render(
        &mut self,
        encoder: &mut astrelis_gpu::CommandEncoder,
        target: ViewRenderTarget,
        camera: &Camera3D,
        list: &DrawList3D,
    ) -> rxui::Result<RenderStats> {
        let stats = match target {
            ViewRenderTarget::Direct(target) => self.renderer.render_composited(
                encoder,
                &target,
                camera,
                &Lighting::default(),
                list,
            ),
            ViewRenderTarget::Texture(target) => {
                self.renderer
                    .render(encoder, &target, camera, &Lighting::default(), list)
            }
        }?;
        Ok(stats)
    }
}

struct RobotArm {
    window: Option<WindowId>,
    ui: Option<UiHandles>,
    scene_id: CompositorViewId,
    scene: Option<SceneGpu>,
    pose: JointPose,
    selected: usize,
    orbit: OrbitCamera,
    dragging: bool,
    last_pointer: Option<(f32, f32)>,
    last_scene_stats: RenderStats,
    last_composition_stats: CompositionStats,
}

impl RobotArm {
    fn new() -> Self {
        Self {
            window: None,
            ui: None,
            scene_id: CompositorViewId::new(),
            scene: None,
            pose: HOME_POSE,
            selected: 0,
            orbit: OrbitCamera::default(),
            dragging: false,
            last_pointer: None,
            last_scene_stats: RenderStats::default(),
            last_composition_stats: CompositionStats::default(),
        }
    }

    fn build_ui(&self) -> rxui::Result<(Ui<Message>, UiHandles)> {
        let mut ui = Ui::new(
            astrelis_ui_core::deterministic_font_database(),
            Theme {
                font_families: vec![astrelis_text::FontFamily::Named("Noto Sans".into())],
                ..Theme::dark()
            },
        );
        let root = ui.root();
        let outer = SplitPane::new(
            &mut ui,
            root,
            SplitPaneOptions {
                axis: SplitAxis::Horizontal,
                ratio: 0.22,
                first_min: 220.0,
                second_min: 620.0,
                ..Default::default()
            },
        )?;
        outer.set_container_layout(
            &mut ui,
            LayoutStyle {
                width: Length::Percent(1.0),
                height: Length::Percent(1.0),
                grow: 1.0,
                ..Default::default()
            },
        )?;
        let inner = SplitPane::new(
            &mut ui,
            outer.second(),
            SplitPaneOptions {
                axis: SplitAxis::Horizontal,
                ratio: 0.72,
                first_min: 360.0,
                second_min: 260.0,
                ..Default::default()
            },
        )?;
        inner.set_container_layout(
            &mut ui,
            LayoutStyle {
                grow: 1.0,
                width: Length::Percent(1.0),
                height: Length::Percent(1.0),
                ..Default::default()
            },
        )?;

        let left = padded_column(&mut ui, outer.first())?;
        ui.label(left, "Joint controls").finish();
        let mut sliders = Vec::new();
        let mut values = Vec::new();
        for (index, spec) in JOINTS.iter().enumerate() {
            let row = ui.row(left).finish();
            ui.label(row, spec.name).grow(1.0).finish();
            let value = ui
                .label(row, format!("{:+.0}°", self.pose.degrees[index]))
                .finish();
            let slider = ui
                .slider(left, spec.min, spec.max, 1.0, self.pose.degrees[index])
                .width(Length::Percent(1.0))
                .finish();
            ui.set_semantic_label(slider, spec.name)?;
            ui.on_slider(slider, move |event, value| {
                event.emit(Message::Joint(index, value));
            });
            sliders.push(slider);
            values.push(value);
        }
        let home = ui.button(left, "Home Pose").finish();
        let zero = ui.button(left, "Zero Pose").finish();
        let reset = ui.button(left, "Reset Camera").finish();
        ui.on_click(home, |event| event.emit(Message::Home));
        ui.on_click(zero, |event| event.emit(Message::Zero));
        ui.on_click(reset, |event| event.emit(Message::ResetCamera));

        let center = inner.first();
        let view = ui.add_widget(
            center,
            RenderView::new("Robotic arm 3D viewport", Message::View),
        )?;
        ui.update_widget(view, |view| {
            view.set_corner_radius(0.0);
            view.set_content(RenderViewContent::Composited {
                id: self.scene_id,
                prefer_direct: true,
            });
        })?;
        ui.set_layout(
            view,
            LayoutStyle {
                grow: 1.0,
                width: Length::Percent(1.0),
                height: Length::Percent(1.0),
                ..Default::default()
            },
        )?;

        let right = padded_column(&mut ui, inner.second())?;
        ui.label(right, "Telemetry").finish();
        let position = ui.label(right, "Position: —").finish();
        let orientation = ui.label(right, "Yaw / pitch / roll: —").finish();
        let selected = ui.label(right, "Selected joint: —").finish();
        ui.label(right, "Fixed link dimensions").finish();
        ui.label(
            right,
            format!(
                "Base {:.2}\nUpper arm {:.2}\nForearm {:.2}\nWrist {:.2}\nTool {:.2}",
                LINKS.base_height, LINKS.upper_arm, LINKS.forearm, LINKS.wrist, LINKS.tool
            ),
        )
        .finish();
        let frame_stats = ui.label(right, "Last frame: —").finish();

        let handles = UiHandles {
            sliders: sliders.try_into().expect("five joint sliders"),
            values: values.try_into().expect("five joint labels"),
            position,
            orientation,
            selected,
            frame_stats,
        };
        update_telemetry_labels(
            &mut ui,
            &handles,
            self.pose,
            self.selected,
            self.last_scene_stats,
            self.last_composition_stats,
        )?;
        Ok((ui, handles))
    }

    fn set_pose(&mut self, cx: &mut AppCx<'_, Message>, pose: JointPose) -> rxui::Result<()> {
        self.pose = pose.clamped();
        let (Some(window), Some(handles)) = (self.window, &self.ui) else {
            return Ok(());
        };
        let ui = cx.ui(window)?;
        for index in 0..5 {
            ui.set_slider_value(handles.sliders[index], self.pose.degrees[index])?;
            ui.set_label_text(
                handles.values[index],
                format!("{:+.0}°", self.pose.degrees[index]),
            )?;
        }
        self.update_telemetry(cx)
    }

    fn update_telemetry(&self, cx: &mut AppCx<'_, Message>) -> rxui::Result<()> {
        if let (Some(window), Some(handles)) = (self.window, &self.ui) {
            update_telemetry_labels(
                cx.ui(window)?,
                handles,
                self.pose,
                self.selected,
                self.last_scene_stats,
                self.last_composition_stats,
            )?;
        }
        Ok(())
    }

    fn handle_message(
        &mut self,
        cx: &mut AppCx<'_, Message>,
        message: Message,
    ) -> rxui::Result<bool> {
        match message {
            Message::Joint(joint, value) => {
                self.pose.set(joint, value);
                self.selected = joint;
                if let (Some(window), Some(handles)) = (self.window, &self.ui) {
                    cx.ui(window)?.set_label_text(
                        handles.values[joint],
                        format!("{:+.0}°", self.pose.degrees[joint]),
                    )?;
                }
                self.update_telemetry(cx)?;
                Ok(true)
            }
            Message::Home => {
                self.set_pose(cx, HOME_POSE)?;
                Ok(true)
            }
            Message::Zero => {
                self.set_pose(cx, ZERO_POSE)?;
                Ok(true)
            }
            Message::ResetCamera => {
                self.orbit = OrbitCamera::default();
                Ok(true)
            }
            Message::View(event) => Ok(self.handle_view_event(event)),
        }
    }

    fn handle_view_event(&mut self, event: RenderViewEvent) -> bool {
        match event {
            RenderViewEvent::PointerButton {
                position,
                button: PointerButton::Primary,
                state,
                ..
            } => {
                self.dragging = state == ElementState::Pressed;
                self.last_pointer = self
                    .dragging
                    .then_some((position.local.x, position.local.y));
                false
            }
            RenderViewEvent::PointerMoved { position, .. } if self.dragging => {
                let next = (position.local.x, position.local.y);
                if let Some(previous) = self.last_pointer.replace(next) {
                    self.orbit.orbit(next.0 - previous.0, next.1 - previous.1);
                    return true;
                }
                false
            }
            RenderViewEvent::PointerCancelled { .. } => {
                self.dragging = false;
                self.last_pointer = None;
                false
            }
            RenderViewEvent::Scroll { delta, .. } => {
                self.orbit.zoom(delta.y);
                true
            }
            _ => false,
        }
    }
}

fn padded_column(
    ui: &mut Ui<Message>,
    parent: ElementHandle<Column>,
) -> rxui::Result<ElementHandle<Column>> {
    let content = ui
        .padding(parent, Insets::all(16.0))
        .grow(1.0)
        .column()
        .finish();
    ui.set_layout(
        content,
        LayoutStyle {
            width: Length::Percent(1.0),
            ..Default::default()
        },
    )?;
    Ok(content)
}

fn update_telemetry_labels(
    ui: &mut Ui<Message>,
    handles: &UiHandles,
    pose: JointPose,
    selected: usize,
    scene: RenderStats,
    composition: CompositionStats,
) -> rxui::Result<()> {
    let end = end_effector_pose(pose);
    let spec = JOINTS[selected];
    ui.set_label_text(
        handles.position,
        format!(
            "End effector XYZ\nX {:+.3}\nY {:+.3}\nZ {:+.3}",
            end.position.x, end.position.y, end.position.z
        ),
    )?;
    ui.set_label_text(
        handles.orientation,
        format!(
            "Yaw / pitch / roll\n{:+.1}°  {:+.1}°  {:+.1}°",
            end.yaw_pitch_roll.x, end.yaw_pitch_roll.y, end.yaw_pitch_roll.z
        ),
    )?;
    ui.set_label_text(
        handles.selected,
        format!(
            "Selected: {}\nAxis {}  Range {:.0}°…{:.0}°\nValue {:+.0}°",
            spec.name, spec.axis, spec.min, spec.max, pose.degrees[selected]
        ),
    )?;
    ui.set_label_text(
        handles.frame_stats,
        format!(
            "Last frame\nScene draws {} / triangles {}\nUI draws {} / triangles {}\nLayers {} / direct {} / texture {}",
            scene.draw_calls,
            scene.triangles,
            composition.paint.draws,
            composition.paint.triangles,
            composition.ui_layers,
            composition.direct_views,
            composition.texture_views,
        ),
    )?;
    Ok(())
}

impl App for RobotArm {
    type Message = Message;

    fn build(&mut self, cx: &mut AppCx<'_, Message>) -> rxui::Result<()> {
        let (ui, handles) = self.build_ui()?;
        let mut config =
            WindowConfig::new("RXUI robotic arm").clear_color(Color::rgb(0.018, 0.026, 0.052));
        if let Some((width, height)) = initial_window_size() {
            config = config.size(width, height);
        }
        let window = cx.open_window(config, ui)?;
        self.ui = Some(handles);
        self.window = Some(window);
        Ok(())
    }

    fn update(&mut self, cx: &mut AppCx<'_, Message>, message: Message) -> rxui::Result<()> {
        if self.handle_message(cx, message)?
            && let Some(window) = self.window
        {
            cx.invalidate(window);
        }
        Ok(())
    }

    fn window_closed(
        &mut self,
        _cx: &mut AppCx<'_, Message>,
        _window: WindowId,
    ) -> rxui::Result<()> {
        self.scene = None;
        self.window = None;
        Ok(())
    }

    fn render(&mut self, cx: &mut AppCx<'_, Message>, window: WindowId) -> rxui::Result<()> {
        if self.scene.is_none() {
            let host = cx.host(window)?;
            let device = host.device().cloned();
            let queue = host.queue().cloned();
            if let (Some(device), Some(queue)) = (device, queue) {
                self.scene = Some(SceneGpu::new(device, queue)?);
            }
        }
        let camera = self.orbit.camera();
        let scene_id = self.scene_id;
        let mut scene_stats = self.last_scene_stats;
        let pose = self.pose;
        let Some(scene) = &mut self.scene else {
            return cx.present(window);
        };
        let draws = scene.draw_list(pose);
        let composition = cx.host(window)?.redraw_composited(
            |_| ViewOptions {
                clear_color: Color::rgb(0.018, 0.026, 0.052),
            },
            |id, encoder, target| {
                if id != scene_id {
                    return Err(rxui::Error::msg(format!(
                        "unknown compositor view {}",
                        id.get()
                    )));
                }
                scene_stats = scene.render(encoder, target, &camera, &draws)?;
                Ok(())
            },
        )?;
        if let Some(composition) = composition
            && (scene_stats != self.last_scene_stats || composition != self.last_composition_stats)
        {
            self.last_scene_stats = scene_stats;
            self.last_composition_stats = composition;
            self.update_telemetry(cx)?;
            cx.invalidate(window);
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn initial_window_size() -> Option<(f64, f64)> {
    Some((1320.0, 780.0))
}

#[cfg(target_arch = "wasm32")]
fn initial_window_size() -> Option<(f64, f64)> {
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> MainResult {
    run(RobotArm::new())
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
/// Starts the robotic-arm editor in the host page canvas.
pub fn start() -> Result<(), wasm_bindgen::JsValue> {
    use wasm_bindgen::JsCast;

    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| wasm_bindgen::JsValue::from_str("browser document is unavailable"))?;
    let canvas = document
        .get_element_by_id("rxui-canvas")
        .ok_or_else(|| wasm_bindgen::JsValue::from_str("#rxui-canvas was not found"))?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| wasm_bindgen::JsValue::from_str("#rxui-canvas is not a canvas"))?;
    rxui::app::spawn_on_canvas(RobotArm::new(), AppConfig::default(), canvas)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))
}

#[cfg(test)]
mod tests {
    use astrelis_ui_core::{SemanticAction, SemanticRole};
    use astrelis_ui_testing::UiHarness;

    use super::*;

    fn close(left: Vec3, right: Vec3) {
        assert!((left - right).length() < 1.0e-4, "{left:?} != {right:?}");
    }

    #[test]
    fn zero_pose_is_a_straight_transform_chain() {
        let transforms = forward_kinematics(ZERO_POSE);
        close(
            transforms.end_effector.transform_point3(Vec3::ZERO),
            Vec3::Y
                * (LINKS.base_height + LINKS.upper_arm + LINKS.forearm + LINKS.wrist + LINKS.tool),
        );
        close(
            transforms.elbow.transform_point3(Vec3::ZERO),
            Vec3::Y * (LINKS.base_height + LINKS.upper_arm),
        );
    }

    #[test]
    fn home_pose_and_end_effector_are_stable() {
        let end = end_effector_pose(HOME_POSE);
        close(end.position, Vec3::new(-0.033_325, 4.308_511, 0.012_129));
        close(
            end.yaw_pitch_roll,
            Vec3::new(34.782_166, -2.575_938, 9.665_795),
        );
    }

    #[test]
    fn joint_limits_and_transform_order_are_enforced() {
        let pose = JointPose::new([999.0, -999.0, 999.0, -999.0, 999.0]).clamped();
        assert_eq!(pose.degrees, [180.0, -90.0, 135.0, -120.0, 180.0]);
        let yawed = forward_kinematics(JointPose::new([90.0, 90.0, 0.0, 0.0, 0.0]));
        close(
            yawed.elbow.transform_point3(Vec3::ZERO),
            Vec3::new(0.0, LINKS.base_height, LINKS.upper_arm),
        );
    }

    #[test]
    fn orbit_and_zoom_are_clamped() {
        let mut orbit = OrbitCamera::default();
        orbit.orbit(0.0, 10_000.0);
        assert_eq!(orbit.elevation, 85.0);
        orbit.orbit(0.0, -10_000.0);
        assert_eq!(orbit.elevation, -10.0);
        orbit.zoom(100_000.0);
        assert_eq!(orbit.distance, 3.0);
        orbit.zoom(-100_000.0);
        assert_eq!(orbit.distance, 16.0);
    }

    #[test]
    fn controls_are_labeled_keyboard_operable_and_update_telemetry() {
        let demo = RobotArm::new();
        let (ui, _handles) = demo.build_ui().unwrap();
        let mut harness = UiHarness::new(ui);
        for spec in JOINTS {
            let slider = harness
                .find(SemanticRole::Slider, spec.name)
                .unwrap()
                .expect("labeled slider");
            assert!(slider.focusable);
            assert!(
                slider
                    .actions
                    .contains(&astrelis_ui_core::SemanticActionKind::SetValue)
            );
        }
        for button in ["Home Pose", "Zero Pose", "Reset Camera"] {
            let node = harness
                .find(SemanticRole::Button, button)
                .unwrap()
                .expect("labeled button");
            assert!(node.focusable);
        }
        harness
            .perform(
                SemanticRole::Slider,
                "Elbow pitch",
                SemanticAction::SetValue(42.0),
            )
            .unwrap();
        assert!(matches!(
            harness.drain_messages().next(),
            Some(Message::Joint(2, 42.0))
        ));
        assert!(
            harness
                .semantic_snapshot()
                .unwrap()
                .contains("Selected: Base yaw")
        );
    }
}
