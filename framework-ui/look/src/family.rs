//! A font family token, and the face it stands for on this system.
//!
//! The tokens name the families Nova is designed in (Inter, JetBrains Mono,
//! Instrument Serif), which a system may not have. Qt asks fontconfig for
//! the best match of a missing family, and fontconfig's own rules turn a
//! name with "Mono" in it into the system's monospace face, one with
//! "Serif" into its serif, anything else into its sans. GPUI doesn't: it
//! falls back to a fixed list of sans faces, so a mono token drew
//! proportional digits (IMP-440). A [`Family`] goes to GPUI as the family
//! fontconfig matches, so both shells draw the same face wherever they run.

use fontconfig_sys::constants::FC_FAMILY;
use fontconfig_sys::{
    FcChar8, FcConfigSubstitute, FcDefaultSubstitute, FcFontMatch, FcInit, FcMatchPattern, FcPattern,
    FcPatternAddString, FcPatternCreate, FcPatternDestroy, FcPatternGetString, FcResultMatch,
};
use gpui::SharedString;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::fmt;
use std::ptr;
use std::sync::{Mutex, OnceLock};

/// A font family token: the family asked for. It becomes the face this
/// system draws it with wherever GPUI takes a family (`font_family`,
/// `font`).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Family(&'static str);

impl Family {
    pub const fn named(name: &'static str) -> Self {
        Self(name)
    }

    /// The family asked for, as the tokens name it.
    pub const fn asked(self) -> &'static str {
        self.0
    }

    /// The family this system draws it with: the asked one when it is
    /// there, else fontconfig's match, worked out once per family.
    pub fn resolved(self) -> SharedString {
        static RESOLVED: OnceLock<Mutex<HashMap<&'static str, SharedString>>> = OnceLock::new();
        let resolved = RESOLVED.get_or_init(Mutex::default);
        let Ok(mut resolved) = resolved.lock() else {
            return SharedString::new_static(self.0);
        };
        resolved
            .entry(self.0)
            .or_insert_with(|| best_match(self.0).map_or_else(|| SharedString::new_static(self.0), SharedString::from))
            .clone()
    }
}

/// Written as the asked name in quotes.
impl fmt::Debug for Family {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}", self.0)
    }
}

impl From<Family> for SharedString {
    fn from(family: Family) -> Self {
        family.resolved()
    }
}

/// A pattern fontconfig made, destroyed when dropped.
struct Pattern(*mut FcPattern);

impl Drop for Pattern {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the pattern is fontconfig's, owned here and not used
            // after this.
            unsafe { FcPatternDestroy(self.0) };
        }
    }
}

/// The family of the face fontconfig picks for `family`, after the user's
/// and the system's rules, as Qt's font database asks it. `None` without
/// fontconfig.
fn best_match(family: &str) -> Option<String> {
    let name = CString::new(family).ok()?;
    // SAFETY: takes no arguments; loads the default configuration once.
    if unsafe { FcInit() } == 0 {
        return None;
    }
    // SAFETY: takes no arguments; returns an owned pattern or null.
    let asked = Pattern(unsafe { FcPatternCreate() });
    if asked.0.is_null() {
        return None;
    }
    let mut result = FcResultMatch;
    // SAFETY: the pattern is live and both strings end in NUL; fontconfig
    // copies the name. A null config is the default one FcInit loaded.
    let matched = unsafe {
        FcPatternAddString(asked.0, FC_FAMILY.as_ptr(), name.as_ptr().cast());
        FcConfigSubstitute(ptr::null_mut(), asked.0, FcMatchPattern);
        FcDefaultSubstitute(asked.0);
        Pattern(FcFontMatch(ptr::null_mut(), asked.0, &raw mut result))
    };
    if matched.0.is_null() || result != FcResultMatch {
        return None;
    }
    let mut found: *mut FcChar8 = ptr::null_mut();
    // SAFETY: the match is live; the string it hands back is its own and
    // read before the match is dropped.
    let has = unsafe { FcPatternGetString(matched.0, FC_FAMILY.as_ptr(), 0, &raw mut found) };
    if has != FcResultMatch || found.is_null() {
        return None;
    }
    // SAFETY: fontconfig's strings end in NUL, and this one lives as long
    // as `matched`.
    let found = unsafe { CStr::from_ptr(found.cast()) };
    Some(found.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_family_is_written_as_tokens_qml_writes_it() {
        assert_eq!(format!("{:?}", Family::named("JetBrains Mono")), "\"JetBrains Mono\"");
    }

    #[test]
    fn a_family_resolves_to_a_face_name() {
        // Whatever fonts the test machine has, the name is one fontconfig
        // can find again, or the asked one without fontconfig.
        assert!(!Family::named("JetBrains Mono").resolved().is_empty());
    }
}
