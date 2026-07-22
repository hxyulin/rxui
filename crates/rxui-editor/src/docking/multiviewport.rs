use std::{collections::HashSet, fmt};

use serde::{Deserialize, Serialize};

use super::{DockError, DockLayout, DockNode, DockPlacement, DockTabs, PanelId};

/// Stable application-defined identity for one native docking viewport.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DockViewportId(String);

impl DockViewportId {
    /// Creates a non-empty viewport identity suitable for persistence.
    pub fn new(value: impl Into<String>) -> Result<Self, DockError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(DockError::new("viewport identity must not be empty"));
        }
        Ok(Self(value))
    }

    /// Returns the persisted identity.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DockViewportId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Serializable docking state hosted by one native window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockViewport {
    /// Stable logical identity, independent of the current platform window ID.
    pub id: DockViewportId,
    /// Suggested native window title.
    pub title: String,
    /// Docking layout shown in this viewport.
    pub layout: DockLayout,
}

impl DockViewport {
    /// Creates a viewport around a docking layout.
    pub fn new(id: DockViewportId, title: impl Into<String>, layout: DockLayout) -> Self {
        Self {
            id,
            title: title.into(),
            layout,
        }
    }
}

/// Serializable collection of docking layouts distributed across native windows.
///
/// Platform `WindowId`s are deliberately excluded: the application maps these
/// stable viewport identities to fresh native windows on every run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MultiViewportDockLayout {
    primary: DockViewportId,
    viewports: Vec<DockViewport>,
}

impl MultiViewportDockLayout {
    /// Creates a collection containing its required primary viewport.
    pub fn new(primary: DockViewport) -> Self {
        Self {
            primary: primary.id.clone(),
            viewports: vec![primary],
        }
    }

    /// Returns the primary viewport identity.
    pub const fn primary(&self) -> &DockViewportId {
        &self.primary
    }

    /// Returns viewports in stable creation order.
    pub fn viewports(&self) -> &[DockViewport] {
        &self.viewports
    }

    /// Returns one viewport.
    pub fn viewport(&self, id: &DockViewportId) -> Option<&DockViewport> {
        self.viewports.iter().find(|viewport| &viewport.id == id)
    }

    /// Returns one viewport for layout updates.
    pub fn viewport_mut(&mut self, id: &DockViewportId) -> Option<&mut DockViewport> {
        self.viewports
            .iter_mut()
            .find(|viewport| &viewport.id == id)
    }

    /// Returns the viewport currently owning a panel.
    pub fn owner(&self, panel: &PanelId) -> Option<&DockViewportId> {
        self.viewports
            .iter()
            .find(|viewport| viewport.layout.contains(panel))
            .map(|viewport| &viewport.id)
    }

    /// Adds a viewport after validating global panel uniqueness.
    pub fn add_viewport(&mut self, viewport: DockViewport) -> Result<(), DockError> {
        let original = self.clone();
        self.viewports.push(viewport);
        if let Err(error) = self.validate() {
            *self = original;
            return Err(error);
        }
        Ok(())
    }

    /// Moves a panel to a placement in another viewport atomically.
    pub fn place_panel(
        &mut self,
        panel: PanelId,
        viewport: &DockViewportId,
        placement: DockPlacement,
    ) -> Result<(), DockError> {
        let original = self.clone();
        for candidate in &mut self.viewports {
            candidate.layout.remove_panel(&panel);
        }
        let result = match self.viewport_mut(viewport) {
            Some(target) => target.layout.place_panel(panel, placement),
            None => Err(DockError::new(format!(
                "viewport {viewport} does not exist"
            ))),
        };
        if let Err(error) = result {
            *self = original;
            return Err(error);
        }
        Ok(())
    }

    /// Detaches one visible panel into a newly created native viewport.
    pub fn detach_panel(
        &mut self,
        panel: PanelId,
        viewport: DockViewportId,
        title: impl Into<String>,
    ) -> Result<(), DockError> {
        if self.viewport(&viewport).is_some() {
            return Err(DockError::new(format!(
                "viewport {viewport} already exists"
            )));
        }
        if self.owner(&panel).is_none() {
            return Err(DockError::new(format!("panel {panel} is not visible")));
        }
        for candidate in &mut self.viewports {
            candidate.layout.remove_panel(&panel);
        }
        let tabs = DockTabs::new(vec![panel])?;
        self.viewports.push(DockViewport::new(
            viewport,
            title,
            DockLayout {
                root: Some(DockNode::Tabs(tabs)),
                floating: Vec::new(),
            },
        ));
        Ok(())
    }

    /// Removes an empty secondary viewport.
    pub fn remove_empty_viewport(&mut self, id: &DockViewportId) -> Result<bool, DockError> {
        if id == &self.primary {
            return Err(DockError::new("the primary viewport cannot be removed"));
        }
        let Some(index) = self
            .viewports
            .iter()
            .position(|viewport| &viewport.id == id)
        else {
            return Ok(false);
        };
        if !self.viewports[index].layout.panels().is_empty() {
            return Err(DockError::new("a non-empty viewport cannot be removed"));
        }
        self.viewports.remove(index);
        Ok(true)
    }

    /// Validates unique viewport identities and global panel ownership.
    pub fn validate(&self) -> Result<(), DockError> {
        if self.viewport(&self.primary).is_none() {
            return Err(DockError::new("the primary viewport is missing"));
        }
        let mut viewport_ids = HashSet::new();
        let mut panel_ids = HashSet::new();
        for viewport in &self.viewports {
            if !viewport_ids.insert(&viewport.id) {
                return Err(DockError::new(format!(
                    "duplicate viewport identity {}",
                    viewport.id
                )));
            }
            for panel in viewport.layout.panels() {
                if !panel_ids.insert(panel) {
                    return Err(DockError::new(format!(
                        "panel {panel} appears in multiple viewports"
                    )));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> PanelId {
        PanelId::new(value).unwrap()
    }

    fn viewport(value: &str, panels: &[&str]) -> DockViewport {
        DockViewport::new(
            DockViewportId::new(value).unwrap(),
            value,
            DockLayout {
                root: Some(DockNode::Tabs(
                    DockTabs::new(panels.iter().map(|panel| id(panel)).collect()).unwrap(),
                )),
                floating: Vec::new(),
            },
        )
    }

    #[test]
    fn detaching_and_redocking_preserves_single_ownership() {
        let main = DockViewportId::new("main").unwrap();
        let tools = DockViewportId::new("tools").unwrap();
        let mut layout = MultiViewportDockLayout::new(viewport("main", &["scene", "inspector"]));
        layout
            .detach_panel(id("inspector"), tools.clone(), "Inspector")
            .unwrap();
        assert_eq!(layout.owner(&id("inspector")), Some(&tools));
        layout
            .place_panel(
                id("inspector"),
                &main,
                DockPlacement::Tab {
                    anchor: id("scene"),
                    index: 1,
                },
            )
            .unwrap();
        assert_eq!(layout.owner(&id("inspector")), Some(&main));
        assert!(layout.viewport(&tools).unwrap().layout.panels().is_empty());
        assert!(layout.remove_empty_viewport(&tools).unwrap());
        layout.validate().unwrap();
    }

    #[test]
    fn duplicate_panels_across_viewports_are_rejected_atomically() {
        let mut layout = MultiViewportDockLayout::new(viewport("main", &["scene"]));
        assert!(layout.add_viewport(viewport("tools", &["scene"])).is_err());
        assert_eq!(layout.viewports().len(), 1);
    }

    #[test]
    fn missing_destination_does_not_remove_the_panel() {
        let mut layout = MultiViewportDockLayout::new(viewport("main", &["scene"]));
        assert!(
            layout
                .place_panel(
                    id("scene"),
                    &DockViewportId::new("missing").unwrap(),
                    DockPlacement::Root { index: 0 },
                )
                .is_err()
        );
        assert_eq!(layout.owner(&id("scene")), Some(layout.primary()));
    }
}
