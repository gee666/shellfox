use fontconfig_sys as fc;
use std::{ffi::CStr, ptr::null_mut, sync::OnceLock};

pub(crate) fn system_ui() -> Option<&'static str> {
    static FAMILY: OnceLock<Option<String>> = OnceLock::new();
    FAMILY.get_or_init(|| match_family(c"system-ui")).as_deref()
}

pub(crate) fn monospace() -> Option<&'static str> {
    static FAMILY: OnceLock<Option<String>> = OnceLock::new();
    FAMILY.get_or_init(|| match_family(c"monospace")).as_deref()
}

struct Pattern(&'static fc::Fc, *mut fc::FcPattern);
impl Drop for Pattern {
    fn drop(&mut self) {
        // SAFETY: a Pattern owns one non-null Fontconfig result.
        unsafe { (self.0.FcPatternDestroy)(self.1) }
    }
}

fn match_family(family: &CStr) -> Option<String> {
    let api = fc::statics::LIB_RESULT.as_ref().ok()?;
    // SAFETY: these calls borrow live patterns. Copy the name before dropping them.
    unsafe {
        if (api.FcInit)() == 0 {
            return None;
        }
        let query = (api.FcNameParse)(family.as_ptr().cast());
        if query.is_null() {
            return None;
        }
        let query = Pattern(api, query);
        if (api.FcConfigSubstitute)(null_mut(), query.1, fc::FcMatchPattern) == 0 {
            return None;
        }
        (api.FcDefaultSubstitute)(query.1);
        let mut status = fc::FcResultNoMatch;
        let matched = (api.FcFontMatch)(null_mut(), query.1, &mut status);
        if matched.is_null() {
            return None;
        }
        let matched = Pattern(api, matched);
        let mut name = null_mut();
        if (api.FcPatternGetString)(matched.1, fc::constants::FC_FAMILY.as_ptr(), 0, &mut name)
            != fc::FcResultMatch
            || name.is_null()
        {
            return None;
        }
        CStr::from_ptr(name.cast())
            .to_str()
            .ok()
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
    }
}
