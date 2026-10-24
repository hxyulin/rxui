//! Small parts: tones, separators, badges and key caps.
use super::{Part, part, root_builders};
use crate::{Element, FontWeight, SemanticRole, Shortcut, TextRole, ThemeColor};
use taffy::prelude::{auto, length, percent};

/// Semantic color of a status element, shared by badges, callouts,
/// toasts and field errors. Each tone has a foreground and a subtle background
/// token.
///
/// | Tone | Foreground | Background |
/// | --- | --- | --- |
/// | `Neutral` | `TextMuted` | `ControlHover` |
/// | `Accent` | `Focus` | `AccentSubtle` |
/// | `Danger` | `Danger` | `DangerSubtle` |
/// | `Warning` | `Warning` | `WarningSubtle` |
/// | `Success` | `Success` | `SuccessSubtle` |
///
/// The accent foreground is the focus color: the accent's hue at 3:1 against
/// the window surfaces, which the accent fill itself does not reach for light
/// accents such as yellow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tone {
    /// Secondary information without a status.
    #[default]
    Neutral,
    /// Highlighted or new information.
    Accent,
    /// Errors and destructive outcomes.
    Danger,
    /// Problems that need attention but do not block.
    Warning,
    /// Completed or passing states.
    Success,
}
impl Tone {
    /// Text and icon color of this tone.
    pub(crate) fn foreground(self) -> ThemeColor {
        match self {
            Self::Neutral => ThemeColor::TextMuted,
            Self::Accent => ThemeColor::Focus,
            Self::Danger => ThemeColor::Danger,
            Self::Warning => ThemeColor::Warning,
            Self::Success => ThemeColor::Success,
        }
    }
    /// Subtle fill behind this tone's foreground. The neutral fill is the hovered
    /// control fill, as the resting control fill equals the light surface.
    pub(crate) fn background(self) -> ThemeColor {
        match self {
            Self::Neutral => ThemeColor::ControlHover,
            Self::Accent => ThemeColor::AccentSubtle,
            Self::Danger => ThemeColor::DangerSubtle,
            Self::Warning => ThemeColor::WarningSubtle,
            Self::Success => ThemeColor::SuccessSubtle,
        }
    }
}

/// A 1-unit [`ThemeColor::Divider`] line between groups of content; see
/// [`separator`].
#[must_use = "add the separator to a row or column"]
pub struct Separator {
    root: Element,
}
root_builders!(Separator);

/// A 1-unit [`ThemeColor::Divider`] line with the [`SemanticRole::Separator`]
/// role. It is not focusable.
///
/// Its orientation follows its flex parent: in a column it is a horizontal line
/// across the column's width, and in a row it is a vertical line
/// [`crate::ThemeMetrics::icon_size`] tall (16 balanced, 14 compact), centered.
/// [`Separator::horizontal`] and [`Separator::vertical`] fix the orientation, for
/// example to inset a line with margins or to place one in a stack.
///
/// ```
/// use rxui::prelude::*;
/// let tools = row().gap(4.)
///     .child(button("Cut")).child(button("Copy"))
///     .child(separator())
///     .child(button("Find"));
/// let panel = column().child(label("General")).child(separator()).child(label("Advanced"));
/// ```
pub fn separator() -> Separator {
    let mut root = part(crate::row(), Part::Separator)
        .background(ThemeColor::Divider)
        .accessibility_role(SemanticRole::Separator);
    // The basis sizes the line along whichever axis is the parent's main axis;
    // the full width only applies across a column, and the part height only
    // across a row.
    root.style.flex_basis = length(1.);
    root.style.size.width = percent(1.);
    root.style.align_self = Some(taffy::AlignSelf::CENTER);
    Separator { root }
}
impl Separator {
    /// A horizontal line 1 unit tall that stretches across a column's width,
    /// minus any horizontal margins.
    pub fn horizontal(mut self) -> Self {
        let style = &mut self.root.style;
        style.flex_basis = auto();
        style.size.width = auto();
        style.align_self = Some(taffy::AlignSelf::STRETCH);
        self.root = self.root.height(1.);
        self
    }
    /// A vertical line 1 unit wide and [`crate::ThemeMetrics::icon_size`] tall,
    /// centered. Set a height, or `fill_height` on a row's child, for a longer
    /// line.
    pub fn vertical(mut self) -> Self {
        let style = &mut self.root.style;
        style.flex_basis = auto();
        style.align_self = Some(taffy::AlignSelf::CENTER);
        self.root = self.root.width(1.);
        self
    }
    /// External spacing on each side; see [`Element::margin`].
    pub fn margin(mut self, value: f32) -> Self {
        self.root = self.root.margin(value);
        self
    }
    /// Horizontal external spacing, such as the inset of a horizontal line.
    pub fn margin_x(mut self, value: f32) -> Self {
        self.root = self.root.margin_x(value);
        self
    }
    /// Vertical external spacing.
    pub fn margin_y(mut self, value: f32) -> Self {
        self.root = self.root.margin_y(value);
        self
    }
}
impl crate::IntoElement for Separator {
    fn into_element(self) -> Element {
        self.root
    }
}

/// A small pill of caption text in a [`Tone`]; see [`badge`].
#[must_use = "add the badge to a parent element"]
pub struct Badge {
    root: Element,
}
root_builders!(Badge);

/// A small pill of medium-weight [`TextRole::Caption`] text, such as a count or
/// a status. It is [`Tone::Neutral`] until [`Badge::tone`] changes it, and it is
/// not focusable.
///
/// A badge is a label, so a button or tab containing it includes its text in
/// its accessible name: a tab captioned "Problems" with a badge "3" is named
/// "Problems 3". Hide the badge with [`Badge::accessibility_hidden`] when the
/// parent's name already says what it shows.
///
/// ```
/// use rxui::prelude::*;
/// let status = row().gap(6.)
///     .child(badge("3 errors").tone(Tone::Danger))
///     .child(badge("Beta").tone(Tone::Accent));
/// ```
pub fn badge(text: impl Into<String>) -> Badge {
    let root = crate::label(text)
        .text_role(TextRole::Caption)
        .font_weight(FontWeight::MEDIUM)
        .padding_x(7.)
        .padding_y(1.)
        // Painting scales radii down to fit, so this draws a full pill.
        .radius(999.);
    Badge { root }.tone(Tone::Neutral)
}
impl Badge {
    /// Selects the foreground and background tokens of `tone`.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.root = self
            .root
            .color(tone.foreground())
            .background(tone.background());
        self
    }
    /// Excludes the badge from semantics and from its parent's accessible name.
    pub fn accessibility_hidden(mut self, hidden: bool) -> Self {
        self.root = self.root.accessibility_hidden(hidden);
        self
    }
}
impl crate::IntoElement for Badge {
    fn into_element(self) -> Element {
        self.root
    }
}

/// A key cap showing `shortcut` with the platform's conventions, from
/// [`Shortcut::label`]: `⇧⌘P` on macOS and `Ctrl+Shift+P` elsewhere.
///
/// It is [`TextRole::Caption`] text in [`ThemeColor::TextMuted`] on the
/// [`ThemeColor::Background`] fill, inset in a 1-unit [`ThemeColor::Border`]
/// outline with the theme's small radius. Assistive technology reads its text.
/// Menu items show their command's shortcut this way and publish it as the
/// item's keyboard shortcut instead.
///
/// ```
/// use rxui::prelude::*;
/// let hint = row().gap(6.)
///     .child(label("Command palette"))
///     .child(kbd(Shortcut::primary("p").shift()));
/// ```
pub fn kbd(shortcut: Shortcut) -> Element {
    part(crate::label(shortcut.label()), Part::KeyCap)
        .text_role(TextRole::Caption)
        .color(ThemeColor::TextMuted)
        .background(ThemeColor::Background)
        .border(1., ThemeColor::Border)
        .padding_x(4.)
        .padding_y(0.)
}
