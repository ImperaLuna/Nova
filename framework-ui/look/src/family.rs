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

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Family(&'static str);

impl Family {
    pub const fn named(name: &'static str) -> Self {
        Self(name)
    }

    pub const fn asked(self) -> &'static str {
        self.0
    }

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

// GPUI falls back to a fixed list of sans faces where fontconfig would match a missing family:
// https://linear.app/imperaluna/issue/IMP-440
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
        assert!(!Family::named("JetBrains Mono").resolved().is_empty());
    }
}
