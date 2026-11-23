//! SIGTERM and SIGHUP end the desktop as the OS would, as in 0.9.x. SIGINT
//! keeps its default.
pub(super) fn listen(request: fn()) {
    let listen = move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(async {
            use tokio::signal::unix::{SignalKind, signal};
            let (Ok(mut terminate), Ok(mut hangup)) = (
                signal(SignalKind::terminate()),
                signal(SignalKind::hangup()),
            ) else {
                return;
            };
            tokio::select! {
                _ = terminate.recv() => {}
                _ = hangup.recv() => {}
            }
            request();
        });
    };
    if let Err(error) = std::thread::Builder::new()
        .name("shellfox-system-end".into())
        .spawn(listen)
    {
        eprintln!("Shellfox signal watch unavailable: {error:#}");
    }
}
