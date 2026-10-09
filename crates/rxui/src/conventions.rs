//! Per-platform interface conventions that are data, not native calls.
//!
//! Everything here is a pure function of the compile target, so headless hosts
//! and tests can use it. Native dialogs already follow the platform themselves;
//! these helpers keep RXUI-drawn dialogs and defaults consistent with them.

/// The role a button plays in a dialog's action row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DialogButtonRole {
    /// The default, confirming action, such as Save or OK.
    Affirmative,
    /// An alternative action, such as "Don't Save".
    Alternative,
    /// Dismisses the dialog without acting.
    Cancel,
}

/// How a platform orders a dialog's action buttons from left to right.
///
/// macOS and GNOME put the affirmative action last, at the trailing edge, with
/// alternative actions furthest from it. Windows (and KDE) put it first.
///
/// ```
/// use rxui::{DialogButtonOrder, DialogButtonRole::*};
/// let buttons = [(Cancel, "Cancel"), (Affirmative, "Save"), (Alternative, "Don't Save")];
/// assert_eq!(DialogButtonOrder::AffirmativeLast.arrange(buttons),
///     ["Don't Save", "Cancel", "Save"]);
/// assert_eq!(DialogButtonOrder::AffirmativeFirst.arrange(buttons),
///     ["Save", "Don't Save", "Cancel"]);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DialogButtonOrder {
    /// Alternative, Cancel, Affirmative: macOS and GNOME.
    AffirmativeLast,
    /// Affirmative, Alternative, Cancel: Windows and KDE.
    AffirmativeFirst,
}
impl DialogButtonOrder {
    /// Order for the compile target: affirmative first on Windows, last elsewhere.
    pub const fn platform() -> Self {
        if cfg!(target_os = "windows") {
            Self::AffirmativeFirst
        } else {
            Self::AffirmativeLast
        }
    }
    /// Sorts buttons by role into this order, keeping the given order within a role.
    pub fn arrange<T>(self, buttons: impl IntoIterator<Item = (DialogButtonRole, T)>) -> Vec<T> {
        let rank = |role| match (self, role) {
            (Self::AffirmativeLast, DialogButtonRole::Alternative) => 0,
            (Self::AffirmativeLast, DialogButtonRole::Cancel) => 1,
            (Self::AffirmativeLast, DialogButtonRole::Affirmative) => 2,
            (Self::AffirmativeFirst, DialogButtonRole::Affirmative) => 0,
            (Self::AffirmativeFirst, DialogButtonRole::Alternative) => 1,
            (Self::AffirmativeFirst, DialogButtonRole::Cancel) => 2,
        };
        let mut buttons: Vec<_> = buttons.into_iter().collect();
        buttons.sort_by_key(|(role, _)| rank(*role));
        buttons.into_iter().map(|(_, button)| button).collect()
    }
}
impl Default for DialogButtonOrder {
    fn default() -> Self {
        Self::platform()
    }
}

/// When a placement draws the focus ring around the focused element.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FocusRingPolicy {
    /// Like CSS `:focus-visible`: after keyboard navigation or typing, and always
    /// on text inputs; a pointer press hides it until the next key. The default,
    /// matching native controls on every desktop platform.
    #[default]
    Keyboard,
    /// Whenever an element has focus, for applications that prefer it always shown.
    Always,
}

/// Font families, in preference order, that the target's own interface uses.
/// The native host substitutes the first one it has loaded for the generic
/// sans-serif default. Names that are not installed are skipped.
pub fn system_ui_font_families() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        // SF Pro ships as the hidden ".SF NS" system face; Helvetica Neue predates it.
        &[
            "SF Pro Text",
            "SF Pro",
            ".SF NS",
            "System Font",
            "Helvetica Neue",
        ]
    } else if cfg!(target_os = "windows") {
        &["Segoe UI Variable Text", "Segoe UI Variable", "Segoe UI"]
    } else {
        // GNOME, Ubuntu, KDE and common distribution defaults.
        &[
            "Cantarell",
            "Adwaita Sans",
            "Ubuntu",
            "Noto Sans",
            "Inter",
            "DejaVu Sans",
            "Liberation Sans",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_order_is_stable_within_a_role_and_matches_the_platform() {
        use DialogButtonRole::*;
        let buttons = [
            (Alternative, 1),
            (Cancel, 2),
            (Alternative, 3),
            (Affirmative, 4),
        ];
        assert_eq!(
            DialogButtonOrder::AffirmativeLast.arrange(buttons),
            [1, 3, 2, 4]
        );
        assert_eq!(
            DialogButtonOrder::AffirmativeFirst.arrange(buttons),
            [4, 1, 3, 2]
        );
        assert_eq!(
            DialogButtonOrder::default() == DialogButtonOrder::AffirmativeFirst,
            cfg!(target_os = "windows")
        );
        assert!(!system_ui_font_families().is_empty());
    }
}
