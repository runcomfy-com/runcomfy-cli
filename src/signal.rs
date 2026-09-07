//! Cross-platform SIGINT (Ctrl-C) receiver shared by every command that
//! waits on a remote job. Fires once per interrupt.
//!
//! On Unix uses `tokio::signal::unix::signal(SIGINT)` so we get every
//! signal even after the first; on Windows falls back to
//! `signal::ctrl_c()` which only fires once per call, so it is re-armed
//! in a loop.
//!
//! If signal registration fails the sender is dropped and `recv()` yields
//! `None` immediately — callers must match `Some(_)` in `tokio::select!`
//! so a failed registration never masquerades as an interrupt.

pub fn sigint_stream() -> tokio::sync::mpsc::UnboundedReceiver<()> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

    #[cfg(unix)]
    tokio::spawn(async move {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sig = match signal(SignalKind::interrupt()) {
            Ok(s) => s,
            Err(_) => return,
        };
        while sig.recv().await.is_some() {
            if tx.send(()).is_err() {
                break;
            }
        }
    });

    #[cfg(not(unix))]
    tokio::spawn(async move {
        loop {
            if tokio::signal::ctrl_c().await.is_err() {
                break;
            }
            if tx.send(()).is_err() {
                break;
            }
        }
    });

    rx
}
