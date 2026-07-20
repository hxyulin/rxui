//! Astreon 0.4 reference 2D scene editor.

#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    io,
    time::{Duration, Instant},
};

use astrelis_app::{App, AppContext, Runtime, RuntimeConfig, TimerId};
use astrelis_core::geometry::{LogicalPoint, Physical, Size};
use astrelis_platform::{ElementState, PointerButton, WindowAttributes, WindowEvent, WindowId};
use astrelis_text::FontDatabase;
use astrelis_ui_core::{ElementHandle, LayoutStyle, Length, Theme, Ui};
use astreon::app::gpu::{
    Extent3d, Texture, TextureCopy, TextureDataLayout, TextureDescriptor, TextureDimension,
    TextureFormat, TextureUsages,
};
use astreon::editor::docking::{
    DockAction, DockAxis, DockLayout, DockNode, DockSide, DockStyle, DockTabs, DockWorkspace,
    PanelDescriptor, PanelId, PreferredPlacement,
};
use astreon::editor::widgets::{RenderView, RenderViewContent, RenderViewEvent};
use astreon::prelude::*;
use astreon::widgets::ExternalImage;
use serde::{Deserialize, Serialize};

const SCENE_WIDTH: u32 = 640;
const SCENE_HEIGHT: u32 = 480;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum EntityKind {
    Rectangle,
    Light,
    Camera,
}

impl EntityKind {
    const fn index(self) -> usize {
        match self {
            Self::Rectangle => 0,
            Self::Light => 1,
            Self::Camera => 2,
        }
    }
    const fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Light,
            2 => Self::Camera,
            _ => Self::Rectangle,
        }
    }
    const fn label(self) -> &'static str {
        match self {
            Self::Rectangle => "Rectangle",
            Self::Light => "Light",
            Self::Camera => "Camera",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Entity {
    id: u64,
    parent: Option<u64>,
    name: String,
    kind: EntityKind,
    x: f64,
    y: f64,
    visible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum InspectorId {
    General,
    Transform,
    Name,
    Kind,
    Visible,
    X,
    Y,
}

#[derive(Clone, Debug)]
enum Message {
    Dock(DockAction),
    Tree(TreeAction<u64>),
    Table(TableAction<u64, &'static str>),
    Property(PropertyAction<InspectorId>),
    View(RenderViewEvent),
    Palette(CommandPaletteEvent),
    NewEntity,
    DeleteEntity,
    Undo,
    Redo,
    OpenPalette,
    SaveLayout,
    LoadLayout,
    DeleteLayout,
    ResetLayout,
}

struct EditEntity {
    id: u64,
    before: Entity,
    after: Entity,
}

impl UndoAction<Vec<Entity>, io::Error> for EditEntity {
    fn label(&self) -> &str {
        "Edit entity"
    }
    fn redo(&mut self, state: &mut Vec<Entity>) -> Result<(), io::Error> {
        replace_entity(state, self.id, self.after.clone())
    }
    fn undo(&mut self, state: &mut Vec<Entity>) -> Result<(), io::Error> {
        replace_entity(state, self.id, self.before.clone())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn replace_entity(entities: &mut [Entity], id: u64, value: Entity) -> Result<(), io::Error> {
    let entity = entities
        .iter_mut()
        .find(|entity| entity.id == id)
        .ok_or_else(|| io::Error::other("entity no longer exists"))?;
    *entity = value;
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PersistedEditor {
    window: Option<WindowPlacement>,
    workspace: WorkspaceState,
}

struct SceneTexture {
    texture: Texture,
    background: Vec<u8>,
    background_pan: LogicalPoint,
    background_zoom: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct ViewOutcome {
    selection_changed: bool,
    entities_changed: bool,
    scene_changed: bool,
    commands_changed: bool,
}

struct PerfProfiler {
    enabled: bool,
    samples: BTreeMap<&'static str, (u64, Duration, Duration)>,
    last_report: Instant,
}

impl PerfProfiler {
    fn from_env() -> Self {
        Self {
            enabled: std::env::var_os("ASTREON_PERF").is_some(),
            samples: BTreeMap::new(),
            last_report: Instant::now(),
        }
    }

    fn record(&mut self, label: &'static str, elapsed: Duration) {
        if !self.enabled {
            return;
        }
        let sample = self.samples.entry(label).or_default();
        sample.0 += 1;
        sample.1 += elapsed;
        sample.2 = sample.2.max(elapsed);
        if self.last_report.elapsed() >= Duration::from_secs(1) {
            for (label, (count, total, max)) in std::mem::take(&mut self.samples) {
                eprintln!(
                    "[perf] {label}: count={count} avg={:.3}ms max={:.3}ms",
                    total.as_secs_f64() * 1000.0 / count as f64,
                    max.as_secs_f64() * 1000.0,
                );
            }
            self.last_report = Instant::now();
        }
    }
}

#[derive(Clone, Debug)]
enum SceneDragTarget {
    View { start_pan: LogicalPoint },
    Entity(Entity),
}

#[derive(Clone, Debug)]
struct SceneDrag {
    origin: LogicalPoint,
    distance: f32,
    target: SceneDragTarget,
}

struct ReferenceEditor {
    graphics: GraphicsContext,
    host: Option<WindowHost<Message>>,
    workspace: Option<DockWorkspace<Message>>,
    tree: Option<TreeView<u64, Message>>,
    table: Option<TableView<u64, &'static str, Message>>,
    properties: Option<PropertyGrid<InspectorId, Message>>,
    palette: Option<CommandPalette<Message>>,
    palette_state: CommandPaletteState,
    render_view: Option<ElementHandle<RenderView<Message>>>,
    scene_texture: Option<SceneTexture>,
    toolbar: Option<Toolbar<Message>>,
    commands: CommandRegistry<Message>,
    router: CommandRouter,
    entities: Vec<Entity>,
    selected: Option<u64>,
    expanded: BTreeSet<u64>,
    expanded_properties: BTreeSet<InspectorId>,
    sort: TableSort<&'static str>,
    columns: Vec<TableColumn<&'static str>>,
    undo: UndoStack<Vec<Entity>, io::Error>,
    pan: LogicalPoint,
    zoom: f32,
    drag: Option<SceneDrag>,
    next_id: u64,
    default_layout: DockLayout,
    workspace_state: WorkspaceState,
    placement: WindowPlacementTracker,
    saved_window: Option<WindowPlacement>,
    store: Option<JsonStateStore>,
    save_timer: Option<TimerId>,
    profiler: PerfProfiler,
}

impl ReferenceEditor {
    fn new() -> Self {
        let default_layout = default_layout();
        let store = JsonStateStore::for_app("dev", "Astreon", "ReferenceEditor").ok();
        let saved = store
            .as_ref()
            .and_then(|store| store.load::<PersistedEditor>(1).ok().flatten());
        let workspace_state = saved
            .as_ref()
            .map(|saved| saved.workspace.clone())
            .filter(|state| state.validate().is_ok())
            .unwrap_or_else(|| WorkspaceState::new(default_layout.clone()));
        let saved_window = saved.as_ref().and_then(|saved| saved.window.clone());
        let placement = saved_window.clone().map_or_else(
            WindowPlacementTracker::default,
            WindowPlacementTracker::from_placement,
        );
        let mut commands = CommandRegistry::new();
        let definitions = [
            (
                "scene.new",
                "New Entity",
                Message::NewEntity,
                Some(Shortcut::primary("n")),
            ),
            ("scene.delete", "Delete Entity", Message::DeleteEntity, None),
            (
                "edit.undo",
                "Undo",
                Message::Undo,
                Some(Shortcut::primary("z")),
            ),
            (
                "edit.redo",
                "Redo",
                Message::Redo,
                Some(Shortcut::primary("y")),
            ),
            (
                "view.palette",
                "Command Palette",
                Message::OpenPalette,
                Some(Shortcut::primary("p")),
            ),
            (
                "layout.save",
                "Save Layout as Workspace 1",
                Message::SaveLayout,
                None,
            ),
            ("layout.load", "Load Workspace 1", Message::LoadLayout, None),
            (
                "layout.delete",
                "Delete Workspace 1",
                Message::DeleteLayout,
                None,
            ),
            ("layout.reset", "Reset Layout", Message::ResetLayout, None),
        ];
        for (id, label, message, shortcut) in definitions {
            let mut command = Command::new(CommandId::new(id).unwrap(), label, message)
                .description(format!("Reference editor: {label}"));
            if let Some(shortcut) = shortcut {
                command = command.shortcut(shortcut);
            }
            commands.register(command).unwrap();
        }
        Self {
            graphics: GraphicsContext::new(),
            host: None,
            workspace: None,
            tree: None,
            table: None,
            properties: None,
            palette: None,
            palette_state: CommandPaletteState::default(),
            render_view: None,
            scene_texture: None,
            toolbar: None,
            commands,
            router: CommandRouter::new(),
            entities: vec![
                Entity {
                    id: 1,
                    parent: None,
                    name: "Camera".into(),
                    kind: EntityKind::Camera,
                    x: -120.0,
                    y: -60.0,
                    visible: true,
                },
                Entity {
                    id: 2,
                    parent: None,
                    name: "World".into(),
                    kind: EntityKind::Rectangle,
                    x: 0.0,
                    y: 0.0,
                    visible: true,
                },
                Entity {
                    id: 3,
                    parent: Some(2),
                    name: "Key Light".into(),
                    kind: EntityKind::Light,
                    x: 120.0,
                    y: -80.0,
                    visible: true,
                },
            ],
            selected: Some(2),
            expanded: BTreeSet::from([2]),
            expanded_properties: BTreeSet::from([InspectorId::General, InspectorId::Transform]),
            sort: TableSort {
                column: "name",
                direction: SortDirection::Ascending,
            },
            columns: vec![
                TableColumn::new("name", "Name", 180.0),
                TableColumn::new("kind", "Kind", 100.0),
                TableColumn::new("visible", "Visible", 80.0),
            ],
            undo: UndoStack::new(100),
            pan: LogicalPoint::ZERO,
            zoom: 1.0,
            drag: None,
            next_id: 4,
            default_layout,
            workspace_state,
            placement,
            saved_window,
            store,
            save_timer: None,
            profiler: PerfProfiler::from_env(),
        }
    }

    fn sync_tree(&mut self) -> Result<(), io::Error> {
        let tree_nodes = build_tree(&self.entities, None, &self.expanded);
        let host = self.host.as_mut().expect("host exists");
        self.tree
            .as_mut()
            .expect("tree")
            .sync(host.ui_mut(), &tree_nodes, self.selected.as_ref())
            .map_err(io::Error::other)
    }

    fn sync_table(&mut self) -> Result<(), io::Error> {
        let mut rows = self
            .entities
            .iter()
            .map(|entity| TableRow {
                id: entity.id,
                cells: vec![
                    entity.name.clone(),
                    entity.kind.label().into(),
                    if entity.visible {
                        "Yes".into()
                    } else {
                        "No".into()
                    },
                ],
            })
            .collect::<Vec<_>>();
        let column = self.sort.column;
        rows.sort_by(|left, right| {
            let index = match column {
                "kind" => 1,
                "visible" => 2,
                _ => 0,
            };
            let order = left.cells[index].cmp(&right.cells[index]);
            if self.sort.direction == SortDirection::Ascending {
                order
            } else {
                order.reverse()
            }
        });
        let host = self.host.as_mut().expect("host exists");
        self.table
            .as_mut()
            .expect("table")
            .sync(
                host.ui_mut(),
                &self.columns,
                &rows,
                Some(&self.sort),
                self.selected.as_ref(),
            )
            .map_err(io::Error::other)
    }

    fn sync_properties(&mut self) -> Result<(), io::Error> {
        let sections = self
            .selected
            .and_then(|id| self.entities.iter().find(|entity| entity.id == id))
            .map(|entity| property_sections(entity, &self.expanded_properties))
            .unwrap_or_default();
        let host = self.host.as_mut().expect("host exists");
        self.properties
            .as_mut()
            .expect("properties")
            .sync(host.ui_mut(), &sections)
            .map_err(io::Error::other)
    }

    fn sync_palette(&mut self) -> Result<(), io::Error> {
        let host = self.host.as_mut().expect("host exists");
        self.palette
            .as_mut()
            .expect("palette")
            .sync(host.ui_mut(), &self.commands, &self.palette_state)
            .map_err(io::Error::other)
    }

    fn sync_commands(&mut self) -> Result<(), io::Error> {
        sync_undo_commands(&mut self.commands, &self.undo);
        let host = self.host.as_mut().expect("host exists");
        self.toolbar
            .as_ref()
            .expect("toolbar")
            .sync(host.ui_mut(), &self.commands)
            .map_err(io::Error::other)
    }

    fn sync_views(&mut self) -> Result<(), io::Error> {
        self.sync_tree()?;
        self.sync_table()?;
        self.sync_properties()?;
        self.sync_palette()?;
        self.sync_commands()
    }

    fn upload_scene(&mut self) -> Result<(), io::Error> {
        let Some(host) = &mut self.host else {
            return Ok(());
        };
        let Some(scene) = &mut self.scene_texture else {
            return Ok(());
        };
        if scene.background.is_empty()
            || scene.background_pan != self.pan
            || scene.background_zoom != self.zoom
        {
            scene.background = render_scene_background(self.pan, self.zoom);
            scene.background_pan = self.pan;
            scene.background_zoom = self.zoom;
        }
        let mut pixels = scene.background.clone();
        for entity in self.entities.iter().filter(|entity| entity.visible) {
            let sx = ((entity.x as f32 - self.pan.x) * self.zoom + SCENE_WIDTH as f32 * 0.5) as i32;
            let sy =
                ((entity.y as f32 - self.pan.y) * self.zoom + SCENE_HEIGHT as f32 * 0.5) as i32;
            let size = (22.0 * self.zoom).clamp(8.0, 50.0) as i32;
            let selected = self.selected == Some(entity.id);
            let color = match entity.kind {
                EntityKind::Rectangle => [70, 140, 230, 255],
                EntityKind::Light => [245, 194, 66, 255],
                EntityKind::Camera => [170, 100, 220, 255],
            };
            fill_rect(
                &mut pixels,
                sx - size,
                sy - size,
                sx + size,
                sy + size,
                color,
            );
            if selected {
                stroke_rect(
                    &mut pixels,
                    sx - size - 3,
                    sy - size - 3,
                    sx + size + 3,
                    sy + size + 3,
                    [255, 255, 255, 255],
                );
            }
        }
        host.queue()
            .expect("GPU is ready on native")
            .write_texture(
                &TextureCopy {
                    texture: scene.texture.clone(),
                    mip_level: 0,
                    origin: Default::default(),
                },
                &pixels,
                TextureDataLayout {
                    offset: 0,
                    bytes_per_row: Some(SCENE_WIDTH * 4),
                    rows_per_image: Some(SCENE_HEIGHT),
                },
                Extent3d::d2(SCENE_WIDTH, SCENE_HEIGHT),
            )
            .map_err(io::Error::other)
    }

    fn apply(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        message: Message,
    ) -> Result<(), io::Error> {
        let mut scene_changed = false;
        let mut sync_tree = false;
        let mut sync_table = false;
        let mut sync_properties = false;
        let mut sync_palette = false;
        let mut sync_commands = false;
        match message {
            Message::Dock(action) => {
                let outcome = self
                    .workspace
                    .as_mut()
                    .unwrap()
                    .apply(self.host.as_mut().unwrap().ui_mut(), action)
                    .map_err(io::Error::other)?;
                if outcome.layout_changed {
                    self.workspace_state.layout = self.workspace.as_ref().unwrap().layout().clone();
                    self.schedule_save(context);
                }
            }
            Message::Tree(action) => match action {
                TreeAction::Select(id) | TreeAction::Activate(id) => {
                    if self.selected != Some(id) {
                        self.selected = Some(id);
                        sync_tree = true;
                        sync_table = true;
                        sync_properties = true;
                        scene_changed = true;
                    }
                }
                TreeAction::SetExpanded { id, expanded } => {
                    if expanded {
                        self.expanded.insert(id);
                    } else {
                        self.expanded.remove(&id);
                    }
                    sync_tree = true;
                }
            },
            Message::Table(action) => match action {
                TableAction::Select(id) | TableAction::Activate(id) => {
                    if self.selected != Some(id) {
                        self.selected = Some(id);
                        sync_tree = true;
                        sync_table = true;
                        sync_properties = true;
                        scene_changed = true;
                    }
                }
                TableAction::SetSort(sort) => {
                    self.sort = sort;
                    sync_table = true;
                }
                TableAction::ResizeColumn { column, width } => {
                    if let Some(value) = self.columns.iter_mut().find(|value| value.id == column) {
                        value.width = width;
                        sync_table = true;
                    }
                }
            },
            Message::Property(action) => match action {
                PropertyAction::Change { id, value } => {
                    if let Some(entity_id) = self.selected
                        && let Some(before) = self
                            .entities
                            .iter()
                            .find(|entity| entity.id == entity_id)
                            .cloned()
                    {
                        let mut after = before.clone();
                        apply_property(&mut after, id, value);
                        self.undo.execute(
                            EditEntity {
                                id: entity_id,
                                before,
                                after,
                            },
                            &mut self.entities,
                        )?;
                        sync_tree = true;
                        sync_table = true;
                        sync_properties = true;
                        sync_commands = true;
                        scene_changed = true;
                    }
                }
                PropertyAction::SetSectionExpanded { id, expanded } => {
                    if expanded {
                        self.expanded_properties.insert(id);
                    } else {
                        self.expanded_properties.remove(&id);
                    }
                    sync_properties = true;
                }
            },
            Message::View(event) => {
                let outcome = self.handle_view(event)?;
                sync_tree |= outcome.selection_changed || outcome.entities_changed;
                sync_table |= outcome.selection_changed || outcome.entities_changed;
                sync_properties |= outcome.selection_changed || outcome.entities_changed;
                sync_commands |= outcome.commands_changed;
                scene_changed |= outcome.scene_changed;
            }
            Message::Palette(event) => {
                self.handle_palette(context, event)?;
                sync_palette = true;
            }
            Message::NewEntity => {
                let id = self.next_id;
                self.next_id += 1;
                self.entities.push(Entity {
                    id,
                    parent: None,
                    name: format!("Entity {id}"),
                    kind: EntityKind::Rectangle,
                    x: 0.0,
                    y: 0.0,
                    visible: true,
                });
                self.selected = Some(id);
                sync_tree = true;
                sync_table = true;
                sync_properties = true;
                scene_changed = true;
            }
            Message::DeleteEntity => {
                if let Some(id) = self.selected.take() {
                    self.entities
                        .retain(|entity| entity.id != id && entity.parent != Some(id));
                    sync_tree = true;
                    sync_table = true;
                    sync_properties = true;
                    scene_changed = true;
                }
            }
            Message::Undo => {
                self.undo.undo(&mut self.entities)?;
                sync_tree = true;
                sync_table = true;
                sync_properties = true;
                sync_commands = true;
                scene_changed = true;
            }
            Message::Redo => {
                self.undo.redo(&mut self.entities)?;
                sync_tree = true;
                sync_table = true;
                sync_properties = true;
                sync_commands = true;
                scene_changed = true;
            }
            Message::OpenPalette => {
                self.palette_state.open = true;
                self.palette_state.query.clear();
                self.palette_state.selected = 0;
                sync_palette = true;
            }
            Message::SaveLayout => {
                self.workspace_state.layout = self.workspace.as_ref().unwrap().layout().clone();
                self.workspace_state
                    .save_named("Workspace 1")
                    .map_err(io::Error::other)?;
                self.save_state();
            }
            Message::LoadLayout => {
                if self.workspace_state.load_named("Workspace 1") {
                    let layout = self.workspace_state.layout.clone();
                    self.workspace
                        .as_mut()
                        .unwrap()
                        .restore(
                            self.host.as_mut().unwrap().ui_mut(),
                            layout,
                            self.default_layout.clone(),
                        )
                        .map_err(io::Error::other)?;
                    self.save_state();
                }
            }
            Message::DeleteLayout => {
                self.workspace_state.delete_named("Workspace 1");
                self.save_state();
            }
            Message::ResetLayout => {
                self.workspace_state.layout = self.default_layout.clone();
                self.workspace
                    .as_mut()
                    .unwrap()
                    .restore(
                        self.host.as_mut().unwrap().ui_mut(),
                        self.default_layout.clone(),
                        self.default_layout.clone(),
                    )
                    .map_err(io::Error::other)?;
                self.save_state();
            }
        }
        if sync_tree {
            self.sync_tree()?;
        }
        if sync_table {
            self.sync_table()?;
        }
        if sync_properties {
            self.sync_properties()?;
        }
        if sync_palette {
            self.sync_palette()?;
        }
        if sync_commands {
            self.sync_commands()?;
        }
        if scene_changed {
            self.upload_scene()?;
        }
        Ok(())
    }

    fn handle_palette(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        event: CommandPaletteEvent,
    ) -> Result<(), io::Error> {
        match event {
            CommandPaletteEvent::QueryChanged(query) => {
                self.palette_state.query = query;
                self.palette_state.selected = 0;
            }
            CommandPaletteEvent::Navigate(delta) => {
                let count =
                    CommandPalette::<Message>::matches(&self.commands, &self.palette_state.query)
                        .len();
                self.palette_state.navigate(delta, count);
            }
            CommandPaletteEvent::Invoke(id) => {
                self.palette_state.open = false;
                if let Some(message) = self.commands.invoke(&id) {
                    self.apply(context, message)?;
                }
            }
            CommandPaletteEvent::InvokeSelected => {
                let ids =
                    CommandPalette::<Message>::matches(&self.commands, &self.palette_state.query);
                if let Some(id) = ids.get(self.palette_state.selected) {
                    let message = self.commands.invoke(id);
                    self.palette_state.open = false;
                    if let Some(message) = message {
                        self.apply(context, message)?;
                    }
                }
            }
            CommandPaletteEvent::Dismiss => self.palette_state.open = false,
        }
        Ok(())
    }

    fn handle_view(&mut self, event: RenderViewEvent) -> Result<ViewOutcome, io::Error> {
        let mut outcome = ViewOutcome::default();
        match event {
            RenderViewEvent::PointerButton {
                position,
                button: PointerButton::Primary,
                state: ElementState::Pressed,
                ..
            } => {
                let previous_selection = self.selected;
                let entity = hit_entity(&self.entities, self.pan, self.zoom, position.normalized)
                    .and_then(|id| {
                        self.selected = Some(id);
                        self.entities.iter().find(|entity| entity.id == id).cloned()
                    });
                let target = entity.map_or(
                    SceneDragTarget::View {
                        start_pan: self.pan,
                    },
                    SceneDragTarget::Entity,
                );
                self.drag = Some(SceneDrag {
                    origin: position.normalized,
                    distance: 0.0,
                    target,
                });
                outcome.selection_changed = self.selected != previous_selection;
                outcome.scene_changed = outcome.selection_changed;
            }
            RenderViewEvent::PointerMoved { position, .. } => {
                if let Some(mut drag) = self.drag.clone() {
                    let dx = position.normalized.x - drag.origin.x;
                    let dy = position.normalized.y - drag.origin.y;
                    drag.distance = ((dx * SCENE_WIDTH as f32).powi(2)
                        + (dy * SCENE_HEIGHT as f32).powi(2))
                    .sqrt();
                    if drag.distance > 4.0 {
                        let delta = LogicalPoint::new(
                            dx * SCENE_WIDTH as f32 / self.zoom,
                            dy * SCENE_HEIGHT as f32 / self.zoom,
                        );
                        match &drag.target {
                            SceneDragTarget::View { start_pan } => {
                                self.pan =
                                    LogicalPoint::new(start_pan.x - delta.x, start_pan.y - delta.y);
                                outcome.scene_changed = true;
                            }
                            SceneDragTarget::Entity(before) => {
                                if let Some(entity) = self
                                    .entities
                                    .iter_mut()
                                    .find(|entity| entity.id == before.id)
                                {
                                    entity.x = before.x + f64::from(delta.x);
                                    entity.y = before.y + f64::from(delta.y);
                                    outcome.scene_changed = true;
                                }
                            }
                        }
                    }
                    self.drag = Some(drag);
                }
            }
            RenderViewEvent::PointerButton {
                button: PointerButton::Primary,
                state: ElementState::Released,
                ..
            } => {
                if let Some(drag) = self.drag.take() {
                    match drag.target {
                        SceneDragTarget::View { .. } if drag.distance < 4.0 => {
                            outcome.selection_changed = self.selected.is_some();
                            self.selected = None;
                            outcome.scene_changed = outcome.selection_changed;
                        }
                        SceneDragTarget::Entity(before) if drag.distance > 4.0 => {
                            if let Some(after) = self
                                .entities
                                .iter()
                                .find(|entity| entity.id == before.id)
                                .cloned()
                                && after != before
                            {
                                self.undo.execute(
                                    EditEntity {
                                        id: before.id,
                                        before,
                                        after,
                                    },
                                    &mut self.entities,
                                )?;
                                outcome.entities_changed = true;
                                outcome.commands_changed = true;
                            }
                        }
                        _ => {}
                    }
                }
            }
            RenderViewEvent::PointerCancelled { .. } => {
                if let Some(SceneDrag {
                    target: SceneDragTarget::Entity(before),
                    ..
                }) = self.drag.take()
                {
                    replace_entity(&mut self.entities, before.id, before)?;
                    outcome.entities_changed = true;
                    outcome.scene_changed = true;
                }
            }
            RenderViewEvent::Scroll { delta, .. } => {
                let zoom = (self.zoom * (1.0 - delta.y * 0.05)).clamp(0.25, 4.0);
                outcome.scene_changed = zoom != self.zoom;
                self.zoom = zoom;
            }
            _ => {}
        }
        Ok(outcome)
    }

    fn save_state(&self) {
        if let Some(store) = &self.store {
            let _ = store.save(
                1,
                &PersistedEditor {
                    window: self.placement.placement().cloned(),
                    workspace: self.workspace_state.clone(),
                },
            );
        }
    }

    fn schedule_save(&mut self, context: &mut AppContext<'_, '_, Self>) {
        if self.save_timer.is_some() {
            return;
        }
        self.save_timer = Some(context.set_timeout(Duration::from_millis(250), |app, _| {
            app.save_timer = None;
            app.save_state();
            Ok(())
        }));
    }
}

impl App for ReferenceEditor {
    type Error = io::Error;
    fn resumed(&mut self, context: &mut AppContext<'_, '_, Self>) -> Result<(), Self::Error> {
        if self.host.is_some() {
            return Ok(());
        }
        let mut ui = Ui::new(FontDatabase::default(), Theme::dark());
        let root = ui.root();
        ui.set_layout(
            root,
            LayoutStyle {
                width: Length::Percent(1.0),
                height: Length::Percent(1.0),
                ..Default::default()
            },
        )
        .map_err(io::Error::other)?;
        let toolbar = Toolbar::new(
            &mut ui,
            root,
            vec![
                ToolbarItem::Command {
                    id: CommandId::new("scene.new").unwrap(),
                    icon: Some(icons::add()),
                },
                ToolbarItem::Command {
                    id: undo_command_id(),
                    icon: Some(icons::undo()),
                },
                ToolbarItem::Command {
                    id: redo_command_id(),
                    icon: Some(icons::redo()),
                },
                ToolbarItem::FlexibleSpace,
                ToolbarItem::Command {
                    id: CommandId::new("view.palette").unwrap(),
                    icon: None,
                },
            ],
            &self.commands,
            ToolbarOptions::default(),
        )
        .map_err(io::Error::other)?;
        let dock_host = ui.add_column(root).map_err(io::Error::other)?;
        ui.set_layout(
            dock_host,
            LayoutStyle {
                grow: 1.0,
                min_height: Length::Px(300.0),
                ..Default::default()
            },
        )
        .map_err(io::Error::other)?;
        let hierarchy_panel = ui.add_column(root).map_err(io::Error::other)?;
        let tree =
            TreeView::new(&mut ui, hierarchy_panel, Message::Tree).map_err(io::Error::other)?;
        ui.set_layout(
            tree.root(),
            LayoutStyle {
                grow: 1.0,
                ..Default::default()
            },
        )
        .map_err(io::Error::other)?;
        let table_panel = ui.add_column(root).map_err(io::Error::other)?;
        let table =
            TableView::new(&mut ui, table_panel, Message::Table).map_err(io::Error::other)?;
        ui.set_layout(
            table.root(),
            LayoutStyle {
                grow: 1.0,
                ..Default::default()
            },
        )
        .map_err(io::Error::other)?;
        let inspector_panel = ui.add_column(root).map_err(io::Error::other)?;
        let properties = PropertyGrid::new(&mut ui, inspector_panel, Message::Property)
            .map_err(io::Error::other)?;
        let scene_panel = ui.add_column(root).map_err(io::Error::other)?;
        let render_view = ui
            .add_widget(
                scene_panel,
                RenderView::new("Interactive 2D scene", Message::View),
            )
            .map_err(io::Error::other)?;
        ui.set_layout(
            render_view,
            LayoutStyle {
                grow: 1.0,
                width: Length::Percent(1.0),
                height: Length::Percent(1.0),
                ..Default::default()
            },
        )
        .map_err(io::Error::other)?;
        let mut workspace = DockWorkspace::new(
            &mut ui,
            dock_host,
            DockStyle {
                divider_size: 4.0,
                divider_visual_size: 1.0,
                ..DockStyle::default()
            },
            Message::Dock,
        )
        .map_err(io::Error::other)?;
        for (id, title, content, closable, preferred) in [
            (
                panel("hierarchy"),
                "Hierarchy",
                hierarchy_panel,
                true,
                PreferredPlacement::Split {
                    anchor: panel("scene"),
                    side: DockSide::Left,
                },
            ),
            (
                panel("scene"),
                "Scene",
                scene_panel,
                false,
                PreferredPlacement::Root,
            ),
            (
                panel("inspector"),
                "Inspector",
                inspector_panel,
                true,
                PreferredPlacement::Split {
                    anchor: panel("scene"),
                    side: DockSide::Right,
                },
            ),
            (
                panel("entities"),
                "Entities",
                table_panel,
                true,
                PreferredPlacement::Split {
                    anchor: panel("scene"),
                    side: DockSide::Bottom,
                },
            ),
        ] {
            let mut descriptor = PanelDescriptor::new(id, title)
                .closable(closable)
                .preferred(preferred);
            if title == "Inspector" {
                descriptor = descriptor.minimum_size(Size::new(240.0, 180.0));
            }
            workspace
                .register_panel(&mut ui, descriptor, content)
                .map_err(io::Error::other)?;
        }
        workspace
            .restore(
                &mut ui,
                self.workspace_state.layout.clone(),
                self.default_layout.clone(),
            )
            .map_err(io::Error::other)?;
        let palette = CommandPalette::new(&mut ui, Message::Palette).map_err(io::Error::other)?;
        let mut attributes = WindowAttributes {
            title: "Astreon 0.4 reference editor".into(),
            inner_size: Some(Size::new(1200.0, 760.0)),
            ..Default::default()
        };
        if let Some(saved) = &self.saved_window {
            let monitors = context.available_monitors();
            let primary = context.primary_monitor();
            saved.apply(&mut attributes, &monitors, primary.as_ref());
        }
        let mut host = WindowHost::open(
            context,
            &self.graphics,
            ui,
            WindowHostOptions {
                window: attributes,
                ..Default::default()
            },
        )
        .map_err(io::Error::other)?;
        let texture = host
            .device()
            .expect("GPU is ready on native")
            .create_texture(TextureDescriptor {
                label: Some("reference editor scene".into()),
                size: Extent3d::d2(SCENE_WIDTH, SCENE_HEIGHT),
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8UnormSrgb,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            });
        let image = ExternalImage::new(Size::<Physical, u32>::new(SCENE_WIDTH, SCENE_HEIGHT))
            .map_err(io::Error::other)?;
        host.register_external_image(&image, texture.create_view(Default::default()))
            .map_err(io::Error::other)?;
        host.ui_mut()
            .update_widget(render_view, |view| {
                view.set_corner_radius(0.0);
                view.set_content(RenderViewContent::Ready {
                    image: image.clone(),
                    source_extent: Size::new(SCENE_WIDTH, SCENE_HEIGHT),
                });
            })
            .map_err(io::Error::other)?;
        self.workspace = Some(workspace);
        self.tree = Some(tree);
        self.table = Some(table);
        self.properties = Some(properties);
        self.palette = Some(palette);
        self.render_view = Some(render_view);
        self.scene_texture = Some(SceneTexture {
            texture,
            background: Vec::new(),
            background_pan: LogicalPoint::ZERO,
            background_zoom: 0.0,
        });
        self.toolbar = Some(toolbar);
        self.host = Some(host);
        self.sync_views()?;
        self.upload_scene()
    }
    fn window_event(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        id: WindowId,
        event: WindowEvent,
    ) -> Result<(), Self::Error> {
        let event_started = Instant::now();
        let shortcut = self.router.handle_event(&event, &self.commands);
        let route_started = Instant::now();
        let update = {
            let host = self.host.as_mut().expect("host");
            self.placement.handle_event(host.window(), &event);
            host.handle_event(&context.clipboard(), &event)
                .map_err(io::Error::other)?
        };
        self.profiler.record("event.route", route_started.elapsed());
        if update.close_requested {
            if let Some(timer) = self.save_timer.take() {
                context.cancel_timer(timer);
            }
            self.placement.capture(self.host.as_ref().unwrap().window());
            self.workspace_state.layout = self.workspace.as_ref().unwrap().layout().clone();
            self.save_state();
            context.unregister_window(id);
            self.host = None;
            context.exit();
            return Ok(());
        }
        if let WindowEvent::Resized(size) = event {
            let width =
                size.width as f32 / self.host.as_ref().unwrap().window().scale_factor() as f32;
            self.toolbar
                .as_ref()
                .unwrap()
                .update_overflow(self.host.as_mut().unwrap().ui_mut(), width)
                .map_err(io::Error::other)?;
        }
        let mut messages = self
            .host
            .as_mut()
            .unwrap()
            .drain_messages()
            .collect::<Vec<_>>();
        if let Some(shortcut) = shortcut {
            messages.push(shortcut);
        }
        for message in messages {
            let label = message_perf_label(&message);
            let apply_started = Instant::now();
            self.apply(context, message)?;
            self.profiler.record(label, apply_started.elapsed());
        }
        if update.redraw
            || self
                .host
                .as_ref()
                .is_some_and(|host| host.ui().needs_redraw())
        {
            context.invalidate_window(id);
        }
        self.profiler.record("event.total", event_started.elapsed());
        Ok(())
    }
    fn redraw(
        &mut self,
        _context: &mut AppContext<'_, '_, Self>,
        _window: WindowId,
    ) -> Result<(), Self::Error> {
        let started = Instant::now();
        if let Some(host) = &mut self.host {
            host.redraw().map_err(io::Error::other)?;
        }
        self.profiler.record("redraw", started.elapsed());
        Ok(())
    }
}

fn message_perf_label(message: &Message) -> &'static str {
    match message {
        Message::Dock(DockAction::SetSplitRatio { .. }) => "apply.dock_resize",
        Message::Dock(_) => "apply.dock",
        Message::Table(TableAction::ResizeColumn { .. }) => "apply.table_resize",
        Message::Table(TableAction::Select(_) | TableAction::Activate(_))
        | Message::Tree(TreeAction::Select(_) | TreeAction::Activate(_)) => "apply.select",
        Message::View(RenderViewEvent::PointerMoved { .. }) => "apply.scene_move",
        Message::View(_) => "apply.scene",
        Message::Property(_) => "apply.property",
        Message::Palette(_) | Message::OpenPalette => "apply.palette",
        _ => "apply.command",
    }
}

fn panel(value: &str) -> PanelId {
    PanelId::new(value).unwrap()
}

fn default_layout() -> DockLayout {
    let center = DockNode::Split {
        axis: DockAxis::Horizontal,
        ratio: 0.2,
        first: Box::new(DockNode::Tabs(
            DockTabs::new(vec![panel("hierarchy")]).unwrap(),
        )),
        second: Box::new(DockNode::Split {
            axis: DockAxis::Horizontal,
            ratio: 0.75,
            first: Box::new(DockNode::Split {
                axis: DockAxis::Vertical,
                ratio: 0.72,
                first: Box::new(DockNode::Tabs(DockTabs::new(vec![panel("scene")]).unwrap())),
                second: Box::new(DockNode::Tabs(
                    DockTabs::new(vec![panel("entities")]).unwrap(),
                )),
            }),
            second: Box::new(DockNode::Tabs(
                DockTabs::new(vec![panel("inspector")]).unwrap(),
            )),
        }),
    };
    DockLayout {
        root: Some(center),
        floating: Vec::new(),
    }
}

fn build_tree(
    entities: &[Entity],
    parent: Option<u64>,
    expanded: &BTreeSet<u64>,
) -> Vec<TreeNode<u64>> {
    entities
        .iter()
        .filter(|entity| entity.parent == parent)
        .map(|entity| {
            TreeNode::leaf(entity.id, &entity.name)
                .expanded(expanded.contains(&entity.id))
                .children(build_tree(entities, Some(entity.id), expanded))
        })
        .collect()
}

fn property_sections(
    entity: &Entity,
    expanded: &BTreeSet<InspectorId>,
) -> Vec<PropertySection<InspectorId>> {
    vec![
        PropertySection {
            id: InspectorId::General,
            title: "General".into(),
            expanded: expanded.contains(&InspectorId::General),
            fields: vec![
                PropertyField {
                    id: InspectorId::Name,
                    label: "Name".into(),
                    value: PropertyValue::Text(entity.name.clone()),
                    validation: if entity.name.trim().is_empty() {
                        ValidationResult::issue(ValidationIssue::error("Name is required"))
                    } else {
                        ValidationResult::valid()
                    },
                    enabled: true,
                },
                PropertyField {
                    id: InspectorId::Kind,
                    label: "Kind".into(),
                    value: PropertyValue::Choice {
                        selected: entity.kind.index(),
                        options: vec!["Rectangle".into(), "Light".into(), "Camera".into()],
                    },
                    validation: ValidationResult::valid(),
                    enabled: true,
                },
                PropertyField {
                    id: InspectorId::Visible,
                    label: "Visible".into(),
                    value: PropertyValue::Boolean(entity.visible),
                    validation: ValidationResult::valid(),
                    enabled: true,
                },
            ],
        },
        PropertySection {
            id: InspectorId::Transform,
            title: "Transform".into(),
            expanded: expanded.contains(&InspectorId::Transform),
            fields: vec![
                PropertyField {
                    id: InspectorId::X,
                    label: "X".into(),
                    value: PropertyValue::Number {
                        value: entity.x,
                        options: NumericFieldOptions {
                            min: -1000.0,
                            max: 1000.0,
                            step: 1.0,
                            decimals: 0,
                        },
                    },
                    validation: ValidationResult::valid(),
                    enabled: true,
                },
                PropertyField {
                    id: InspectorId::Y,
                    label: "Y".into(),
                    value: PropertyValue::Number {
                        value: entity.y,
                        options: NumericFieldOptions {
                            min: -1000.0,
                            max: 1000.0,
                            step: 1.0,
                            decimals: 0,
                        },
                    },
                    validation: ValidationResult::valid(),
                    enabled: true,
                },
            ],
        },
    ]
}

fn apply_property(entity: &mut Entity, id: InspectorId, value: PropertyValue) {
    match (id, value) {
        (InspectorId::Name, PropertyValue::Text(value)) => entity.name = value,
        (InspectorId::Kind, PropertyValue::Choice { selected, .. }) => {
            entity.kind = EntityKind::from_index(selected)
        }
        (InspectorId::Visible, PropertyValue::Boolean(value)) => entity.visible = value,
        (InspectorId::X, PropertyValue::Number { value, .. }) => entity.x = value,
        (InspectorId::Y, PropertyValue::Number { value, .. }) => entity.y = value,
        _ => {}
    }
}

fn hit_entity(
    entities: &[Entity],
    pan: LogicalPoint,
    zoom: f32,
    point: LogicalPoint,
) -> Option<u64> {
    let x = (point.x - 0.5) * SCENE_WIDTH as f32 / zoom + pan.x;
    let y = (point.y - 0.5) * SCENE_HEIGHT as f32 / zoom + pan.y;
    entities
        .iter()
        .rev()
        .find(|entity| {
            entity.visible
                && (entity.x as f32 - x).abs() <= 24.0
                && (entity.y as f32 - y).abs() <= 24.0
        })
        .map(|entity| entity.id)
}

fn render_scene_background(pan: LogicalPoint, zoom: f32) -> Vec<u8> {
    let mut pixels = vec![0_u8; (SCENE_WIDTH * SCENE_HEIGHT * 4) as usize];
    for y in 0..SCENE_HEIGHT {
        for x in 0..SCENE_WIDTH {
            let index = ((y * SCENE_WIDTH + x) * 4) as usize;
            let world_x = (x as f32 - SCENE_WIDTH as f32 * 0.5) / zoom + pan.x;
            let world_y = (y as f32 - SCENE_HEIGHT as f32 * 0.5) / zoom + pan.y;
            let grid = (world_x.round() as i32).rem_euclid(32) <= 1
                || (world_y.round() as i32).rem_euclid(32) <= 1;
            let base = if grid { 40 } else { 27 };
            pixels[index..index + 4].copy_from_slice(&[base, base + 5, base + 12, 255]);
        }
    }
    pixels
}

fn fill_rect(pixels: &mut [u8], left: i32, top: i32, right: i32, bottom: i32, color: [u8; 4]) {
    for y in top.max(0)..bottom.min(SCENE_HEIGHT as i32) {
        for x in left.max(0)..right.min(SCENE_WIDTH as i32) {
            let index = ((y as u32 * SCENE_WIDTH + x as u32) * 4) as usize;
            pixels[index..index + 4].copy_from_slice(&color);
        }
    }
}
fn stroke_rect(pixels: &mut [u8], left: i32, top: i32, right: i32, bottom: i32, color: [u8; 4]) {
    fill_rect(pixels, left, top, right, top + 2, color);
    fill_rect(pixels, left, bottom - 2, right, bottom, color);
    fill_rect(pixels, left, top, left + 2, bottom, color);
    fill_rect(pixels, right - 2, top, right, bottom, color);
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), astrelis_app::RuntimeError<io::Error>> {
    Runtime::finish(astrelis_platform_winit::run_return(Runtime::new(
        ReferenceEditor::new(),
        RuntimeConfig::default(),
    )))
    .map(|_| ())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
