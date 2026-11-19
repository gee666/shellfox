pub(crate) fn family() -> &'static str {
    #[cfg(windows)]
    return "Segoe UI";
    #[cfg(target_os = "linux")]
    return linux::family();
    #[cfg(not(any(windows, target_os = "linux")))]
    ".SystemUIFont"
}

#[cfg(target_os = "linux")]
mod linux {
    use fontconfig_sys as fc;
    use std::{ffi::CStr, ptr::null_mut, sync::OnceLock};

    pub(super) fn family() -> &'static str {
        static FAMILY: OnceLock<Option<String>> = OnceLock::new();
        FAMILY.get_or_init(system_ui).as_deref().unwrap_or(".SystemUIFont")
    }

    struct Pattern(&'static fc::Fc, *mut fc::FcPattern);
    impl Drop for Pattern {
        fn drop(&mut self) {
            // SAFETY: a Pattern owns one non-null Fontconfig result.
            unsafe { (self.0.FcPatternDestroy)(self.1) }
        }
    }

    fn system_ui() -> Option<String> {
        let api = fc::statics::LIB_RESULT.as_ref().ok()?;
        // SAFETY: Fontconfig owns initialization and all matching calls borrow
        // live patterns. We copy the returned family before destroying them.
        unsafe {
            if (api.FcInit)() == 0 { return None; }
            let query = (api.FcNameParse)(c"system-ui".as_ptr().cast());
            if query.is_null() { return None; }
            let query = Pattern(api, query);
            if (api.FcConfigSubstitute)(null_mut(), query.1, fc::FcMatchPattern) == 0 {
                return None;
            }
            (api.FcDefaultSubstitute)(query.1);
            let mut status = fc::FcResultNoMatch;
            let matched = (api.FcFontMatch)(null_mut(), query.1, &mut status);
            if matched.is_null() { return None; }
            let matched = Pattern(api, matched);
            let mut name = null_mut();
            if (api.FcPatternGetString)(matched.1, fc::constants::FC_FAMILY.as_ptr(), 0, &mut name)
                != fc::FcResultMatch || name.is_null() { return None; }
            CStr::from_ptr(name.cast()).to_str().ok().filter(|name| !name.is_empty()).map(str::to_owned)
        }
    }
}
