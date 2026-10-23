use super::{FontSlant, FontStretch, TextError, TextLayout, cosmic};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

static NEXT_SYSTEM: AtomicU64 = AtomicU64::new(1);

/// Opaque face identity, scoped to its originating [`TextSystem`].
/// Equal backend face numbers in different systems are different RXUI drawing IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontId {
    pub(crate) system: u64,
    pub(crate) face: cosmic::fontdb::ID,
}

/// Metadata for one loaded face, including faces from an OpenType collection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontInfo {
    /// Source-scoped identity.
    pub id: FontId,
    /// Family names supplied by the font, without duplicate localized names.
    pub families: Vec<String>,
    /// PostScript name supplied by the font.
    pub postscript_name: String,
    /// Face index within the original file/collection.
    pub face_index: u32,
    /// Reported default weight.
    pub weight: u16,
    /// Reported slope.
    pub slant: FontSlant,
    /// Reported width class.
    pub stretch: FontStretch,
    /// Whether the font reports a fixed pitch.
    pub monospaced: bool,
}

/// Application-owned fonts, font matching caches, and shaping scratch, without a GPU.
///
/// [`Self::new`] starts empty with a deterministic `en-US` fallback locale.
/// Embedded fonts and system discovery are explicit, potentially expensive startup
/// operations. Loading fonts advances a generation and rebuilds backend font/fallback
/// caches, so subsequent buffer evaluation can resolve newly available glyphs.
/// Existing layouts retain their original fonts and remain valid. The optional
/// backend shaped-string cache is disabled; shaped runs belong to retained buffers.
#[derive(Debug)]
pub struct TextSystem {
    pub(crate) backend: cosmic::FontSystem,
    pub(crate) id: u64,
    pub(crate) generation: u64,
    pub(crate) measure: MeasureScratch,
}
/// Reused line-layout storage for [`super::TextBuffer::measure`].
#[derive(Default)]
pub(crate) struct MeasureScratch {
    pub shape: cosmic::ShapeBuffer,
    pub lines: Vec<cosmic::LayoutLine>,
}
impl std::fmt::Debug for MeasureScratch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MeasureScratch").finish_non_exhaustive()
    }
}
impl Default for TextSystem {
    fn default() -> Self {
        Self::new()
    }
}
impl TextSystem {
    /// Creates an empty system without scanning installed fonts, using `en-US`.
    pub fn new() -> Self {
        Self::with_locale("en-US")
    }
    /// Creates an empty system with an explicit fallback locale, without discovery.
    pub fn with_locale(locale: impl Into<String>) -> Self {
        Self {
            backend: cosmic::FontSystem::new_with_locale_and_db(
                locale.into(),
                cosmic::fontdb::Database::new(),
            ),
            id: NEXT_SYSTEM.fetch_add(1, Ordering::Relaxed),
            generation: 0,
            measure: MeasureScratch::default(),
        }
    }
    /// Copies font bytes once and loads all usable faces from a TTF/OTF/collection.
    /// Returns face metadata; invalid data leaves the database/generation unchanged.
    /// Use [`Self::load_font_shared`] to retain an existing allocation without a copy.
    pub fn load_font(&mut self, bytes: &[u8]) -> Result<Vec<FontInfo>, TextError> {
        self.load_font_shared(Arc::from(bytes))
    }
    /// Retains immutable bytes without copying their contents and loads all usable faces.
    /// Unsupported faces are skipped; a file with no usable face returns `InvalidFont`.
    pub fn load_font_shared(&mut self, bytes: Arc<[u8]>) -> Result<Vec<FontInfo>, TextError> {
        let mut db = self.backend.db().clone();
        let ids = db.load_font_source(cosmic::fontdb::Source::Binary(Arc::new(bytes)));
        let ids: Vec<_> = ids.into_iter().collect();
        if ids.is_empty() {
            return Err(TextError::InvalidFont);
        }
        let accepted = self.install(db, ids);
        if accepted.is_empty() {
            return Err(TextError::InvalidFont);
        }
        Ok(accepted)
    }
    /// Discovers installed fonts using the backend's platform policy. This can block.
    /// Returns newly accepted faces. Discovery is best-effort; invalid/unreadable faces
    /// are skipped. With no newly usable faces, the generation stays unchanged.
    pub fn load_system_fonts(&mut self) -> Vec<FontInfo> {
        let mut db = self.backend.db().clone();
        let existing: std::collections::HashSet<_> = db.faces().map(|f| f.id).collect();
        db.load_system_fonts();
        let ids = db
            .faces()
            .map(|f| f.id)
            .filter(|id| !existing.contains(id))
            .collect();
        self.install(db, ids)
    }
    fn install(
        &mut self,
        db: cosmic::fontdb::Database,
        ids: Vec<cosmic::fontdb::ID>,
    ) -> Vec<FontInfo> {
        if ids.is_empty() {
            return Vec::new();
        }
        let mut backend =
            cosmic::FontSystem::new_with_locale_and_db(self.backend.locale().into(), db);
        let mut accepted = Vec::new();
        let mut removed = false;
        for id in ids {
            let weight = backend.db().face(id).unwrap().weight;
            if backend.get_font(id, weight).is_some() {
                accepted.push(id);
            } else {
                backend.db_mut().remove_face(id);
                removed = true;
            }
        }
        if accepted.is_empty() {
            return Vec::new();
        }
        if removed {
            backend = cosmic::FontSystem::new_with_locale_and_db(
                self.backend.locale().into(),
                backend.db().clone(),
            );
        }
        self.backend = backend;
        self.generation += 1;
        accepted
            .into_iter()
            .map(|id| self.info(id).unwrap())
            .collect()
    }
    fn info(&self, id: cosmic::fontdb::ID) -> Option<FontInfo> {
        let face = self.backend.db().face(id)?;
        let mut families = Vec::new();
        for (name, _) in &face.families {
            if !families.contains(name) {
                families.push(name.clone());
            }
        }
        Some(FontInfo {
            id: FontId {
                system: self.id,
                face: id,
            },
            families,
            postscript_name: face.post_script_name.clone(),
            face_index: face.index,
            weight: face.weight.0,
            slant: FontSlant::from_backend(face.style),
            stretch: face.stretch,
            monospaced: face.monospaced,
        })
    }
    /// Iterates loaded face metadata. This allocates names as metadata is requested.
    pub fn fonts(&self) -> impl Iterator<Item = FontInfo> + '_ {
        self.backend
            .db()
            .faces()
            .map(|face| self.info(face.id).unwrap())
    }
    /// Looks up metadata for an identity from this system; foreign IDs return `None`.
    pub fn font_info(&self, id: FontId) -> Option<FontInfo> {
        if id.system != self.id {
            return None;
        }
        self.info(id.face)
    }
    /// Whether a snapshot was produced using this system. Older font generations
    /// still belong to the same source and retain their original selected faces.
    pub fn owns_layout(&self, layout: &TextLayout) -> bool {
        layout.system_id == self.id
    }
    /// Read-only backend access for font discovery/matching inspection.
    /// Mutation stays within managed loading so buffer caches cannot become stale.
    pub fn as_cosmic(&self) -> &cosmic::FontSystem {
        &self.backend
    }
}
