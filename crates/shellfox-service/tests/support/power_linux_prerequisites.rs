// Only skip known D-Bus availability or authorization failures. Unexpected
// acquire/release failures must still fail the test.
pub fn inhibitor_unavailable(error: &str) -> bool {
    error.contains("org.freedesktop.DBus.Error.ServiceUnknown:")
        || error.contains("org.freedesktop.DBus.Error.NameHasNoOwner:")
        || error.contains("org.freedesktop.DBus.Error.InteractiveAuthorizationRequired:")
        || error.contains("org.freedesktop.DBus.Error.AccessDenied:")
        || ["session bus:", "system bus:"].into_iter().any(|bus| {
            error.contains(bus)
                && (error.contains("No such file or directory")
                    || error.contains("Connection refused")
                    || error.contains("$DISPLAY"))
        })
}
