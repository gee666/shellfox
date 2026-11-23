use std::{cell::RefCell, collections::HashMap};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{PostMessageW, WM_CLOSE, WM_ENDSESSION, WM_NCDESTROY},
    },
};

const HOOK: usize = 0x5346_5809;
thread_local! {
    static ENDING: RefCell<HashMap<usize, bool>> = RefCell::new(HashMap::new());
}

pub(super) struct EndSessionHook(HWND);
impl EndSessionHook {
    pub(super) fn install(hwnd: HWND) -> Option<Self> {
        if unsafe { SetWindowSubclass(hwnd, Some(dispatch), HOOK, 0) }.as_bool() {
            ENDING.with(|states| states.borrow_mut().insert(hwnd.0 as usize, false));
            Some(Self(hwnd))
        } else {
            None
        }
    }

    pub(super) fn requested(&self) -> bool {
        ENDING.with(|states| {
            states
                .borrow()
                .get(&(self.0.0 as usize))
                .copied()
                .unwrap_or(false)
        })
    }
}
impl Drop for EndSessionHook {
    fn drop(&mut self) {
        ENDING.with(|states| states.borrow_mut().remove(&(self.0.0 as usize)));
        let _ = unsafe { RemoveWindowSubclass(self.0, Some(dispatch), HOOK) };
    }
}

unsafe extern "system" fn dispatch(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    if message == WM_ENDSESSION && wparam.0 != 0 {
        let tracked = ENDING.with(|states| {
            let mut states = states.borrow_mut();
            states
                .get_mut(&(hwnd.0 as usize))
                .map(|ending| *ending = true)
                .is_some()
        });
        if tracked {
            // Enter the normal close path after returning from the native callback.
            let _ = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            return LRESULT(0);
        }
    } else if message == WM_NCDESTROY {
        ENDING.with(|states| states.borrow_mut().remove(&(hwnd.0 as usize)));
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}
