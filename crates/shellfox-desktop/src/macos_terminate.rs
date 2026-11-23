//! GPUI 0.2.2's app delegate has no `applicationShouldTerminate:`, so NSApp
//! terminates at once. This adds one to GPUI's delegate class before the event
//! loop starts: a request that `allowed` refuses is cancelled and sent to
//! `ask`, which answers with the app's own Quit.
use objc2::{
    ffi::class_addMethod,
    runtime::{AnyClass, AnyObject, Imp, Sel},
    sel,
};
use std::cell::RefCell;
use tokio::sync::mpsc::UnboundedSender;

const TERMINATE_CANCEL: usize = 0;
const TERMINATE_NOW: usize = 1;

/// Whether a terminate may proceed, and where a refused one goes.
type Route = (fn() -> bool, UnboundedSender<()>);
thread_local! {
    static ROUTE: RefCell<Option<Route>> = const { RefCell::new(None) };
}

/// Call on the main thread before GPUI's event loop runs.
pub(crate) fn install(allowed: fn() -> bool, ask: UnboundedSender<()>) {
    ROUTE.with(|route| *route.borrow_mut() = Some((allowed, ask)));
    let Some(class) = AnyClass::get(c"GPUIApplicationDelegate") else {
        eprintln!("GPUI's app delegate is missing; quit requests are not confirmed.");
        return;
    };
    let reply: extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject) -> usize = should_terminate;
    // SAFETY: the function has the method's ABI and matches the type encoding
    // (NSUInteger, self, _cmd, sender). GPUI's class lacks this method, so
    // nothing is replaced; NSApp reads it when the delegate is set in run.
    let added = unsafe {
        class_addMethod(
            (class as *const AnyClass).cast_mut(),
            sel!(applicationShouldTerminate:),
            std::mem::transmute::<
                extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject) -> usize,
                Imp,
            >(reply),
            c"Q@:@".as_ptr(),
        )
    };
    if !added.as_bool() {
        eprintln!("Shellfox could not route quit requests; they are not confirmed.");
    }
}

extern "C-unwind" fn should_terminate(_: &AnyObject, _: Sel, _: *mut AnyObject) -> usize {
    ROUTE.with(|route| match &*route.borrow() {
        Some((allowed, ask)) if !allowed() && ask.send(()).is_ok() => TERMINATE_CANCEL,
        _ => TERMINATE_NOW,
    })
}
