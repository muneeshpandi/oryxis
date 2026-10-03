//! Relaunching Oryxis.
//!
//! The renderer switch relaunches the app in place, handing the master
//! password to the replacement on stdin. Not a shortcut: it lives here
//! because the menu entry that triggers it does, and getting it out of
//! the key router is exactly the point of this split. (A new WINDOW is
//! not a new process: see `window_ctx`.)


use crate::app::Oryxis;

impl Oryxis {
    /// Relaunch the app in place: spawn a fresh process that inherits
    /// the unlocked vault, then exit the current one. Used to apply a
    /// setting that is only read at process start (the graphics
    /// renderer). The child carries `--relaunch` so it waits for this
    /// process's single-instance mutex to release and comes back as
    /// primary. Live SSH sessions and tabs do not survive a process
    /// restart, the caller warns the user before invoking this.
    ///
    /// Never returns on success (`process::exit`). On a spawn failure it
    /// returns so the caller stays running rather than stranding the user
    /// with no window.
    ///
    /// The one exit door that cannot drain the plugin subprocesses:
    /// `process::exit` leaves no runtime to await the drain on. See
    /// `drain_plugins_before_exit`.
    pub(crate) fn relaunch_self(&mut self) {
        // Geometry BEFORE the spawn, on its own, and it is not a
        // duplicate of the `persist_before_exit` below: the replacement
        // reads the window settings in the first lines of `main`, well
        // before the `--relaunch` mutex wait that lets this process
        // finish, so a geometry written after the spawn is a race the
        // child loses. Same crash-safe checkpoint the maximize and
        // focus-loss paths take, for the same reason.
        self.persist_window_geometry();
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("relaunch: current_exe unavailable: {e}");
                return;
            }
        };
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("--relaunch");
        // A diagnostic session started with --debug-log must not lose its
        // log across a renderer-change restart: that restart is exactly
        // the kind of event the log is being kept for.
        if crate::logging::is_forced() {
            cmd.arg("--debug-log");
        }
        let inherit = self.master_password.is_some();
        if inherit {
            cmd.arg("--inherit-vault");
            cmd.stdin(std::process::Stdio::piped());
        }
        match cmd.spawn() {
            Ok(mut child) => {
                if inherit
                    && let Some(mut stdin) = child.stdin.take()
                    && let Some(pw) = self.master_password.as_ref()
                {
                    use std::io::Write as _;
                    let _ = writeln!(stdin, "{}", pw);
                    drop(stdin);
                }
                // The point of no return, and the last place anything
                // can be written: `process::exit` runs no destructor
                // and this door never passes through the close path.
                // AFTER the spawn on purpose. A failed spawn leaves the
                // app running, and a "final" flush on a live pane would
                // hand the recording a tail in the middle of itself.
                self.persist_before_exit();
                // Hand off cleanly: the child is up, drop this process so
                // the mutex releases and the child promotes to primary.
                std::process::exit(0);
            }
            Err(e) => tracing::error!("relaunch: spawn failed: {e}"),
        }
    }
}
