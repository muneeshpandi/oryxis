//! Batch `.ice` runner front-end of the headless harness (CI mode).
//!
//! `oryxis --harness-run <dir>` executes every `.ice` file in `<dir>`
//! in file-name order, each one against a freshly wiped sandbox (the
//! `.oryxis` directory is removed before every test), so tests are
//! independent, deterministic and always start from the first-run
//! (onboarding) state regardless of what ran before them.
//!
//! Unlike `iced_test::run`, which waits indefinitely for the emulator
//! to quiesce after each instruction (a live PTY keeps a never-ending
//! reader task around, so a terminal test would deadlock), this runner
//! drives the shared [`Session`] with its per-instruction timeout: a
//! timed-out instruction still executed and the test moves on, exactly
//! like the REPL and MCP front-ends behave. Synchronization is
//! explicit via the harness grammar.
//!
//! Grammar: every `iced_test` instruction (`click`, `type`, `expect`,
//! ...) plus the harness lines shared with the interactive front-ends:
//!
//! - `settle [idle_ms]`: pump until the event stream stays quiet
//! - `wait <ms>`: pump for a fixed duration
//! - `timeout <ms>`: set the per-instruction timeout (use `timeout
//!   500` once a terminal session is open)
//! - `screenshot [name]`: render a PNG into the shots directory (a CI
//!   artifact for visual review; canvas content such as the terminal
//!   grid is invisible to `expect`, screenshots are how those flows
//!   get validated)
//! - `clipboard "text"` seeds the emulated clipboard, `clipboard is
//!   "text"` ASSERTS it (a mismatch fails the test). Every clipboard
//!   access in the app goes through the iced runtime, so the emulated
//!   clipboard sees the app's own copies too
//! - `find "Text"` / `texts`: walk the widget tree and report. Never
//!   fails, so it is not an assertion (`expect` is). Its other effect
//!   is the one tests actually reach for: the walk REBUILDS the tree,
//!   which is what makes a coordinate click into a just-mounted panel
//!   or a hover-revealed button land on something
//! - blank lines and `#` comments are skipped
//!
//! A failing instruction (target not found / expectation not met)
//! writes `<dir>/errors/<test>.png` plus a reproduction
//! `<dir>/errors/<test>.ice` holding the lines that ran, then exits
//! non-zero.

use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::{Program, Size};
use iced_test::emulator;

use super::{Frontend, Options, Pump, RunOutcome, Session};

/// One parsed `.ice` file: the header metadata plus the raw body
/// lines (comments and blanks already dropped).
struct IceFile {
    path: PathBuf,
    viewport: Size,
    mode: emulator::Mode,
    lines: Vec<String>,
}

pub(super) fn serve<P>(program: P, options: Options, dir: &Path) -> iced::Result
where
    P: Program + 'static,
{
    let tests = match collect(dir) {
        Ok(tests) => tests,
        Err(reason) => fail(&reason),
    };
    if tests.is_empty() {
        fail(&format!("no .ice tests found in {}", dir.display()));
    }

    let errors_dir = dir.join("errors");
    if errors_dir.exists()
        && let Err(e) = std::fs::remove_dir_all(&errors_dir)
    {
        fail(&format!("clearing {}: {e}", errors_dir.display()));
    }

    for test in &tests {
        if let Err(reason) = run_test(&program, &options, test, &errors_dir) {
            fail(&format!(
                "the ice test ({}) failed: {reason}",
                test.path.display()
            ));
        }
        println!(
            "== ok {} ({} lines)",
            test.path.display(),
            test.lines.len()
        );
    }

    println!("== ok all ice tests passed in {}", dir.display());
    Ok(())
}

/// Prints the failure and exits non-zero; a test tool must never
/// mis-report a broken run as green.
fn fail(reason: &str) -> ! {
    eprintln!("oryxis harness: ice run failed: {reason}");
    std::process::exit(1);
}

/// Reads and parses every `.ice` file in `dir`, sorted by file name
/// so the run order is deterministic (`fs::read_dir` order is not).
fn collect(dir: &Path) -> Result<Vec<IceFile>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("ice"))
        .collect();
    paths.sort();

    let mut tests = Vec::with_capacity(paths.len());
    for path in paths {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("reading {}: {e}", path.display()))?;
        let (viewport, mode, lines) =
            parse_ice(&content).map_err(|e| format!("{}: {e}", path.display()))?;
        tests.push(IceFile {
            path,
            viewport,
            mode,
            lines,
        });
    }
    Ok(tests)
}

/// Parses an `.ice` file: `key: value` metadata lines up to the
/// `-----` separator, then body lines. Mirrors `iced_test::Ice`'s
/// header (viewport + mode) without eagerly validating body lines as
/// instructions, since the harness grammar is a superset.
fn parse_ice(content: &str) -> Result<(Size, emulator::Mode, Vec<String>), String> {
    let mut viewport = None;
    let mut mode = None;
    let mut lines = Vec::new();
    let mut in_body = false;

    for (index, raw) in content.lines().enumerate() {
        let line = raw.trim();
        if !in_body {
            if line == "-----" {
                in_body = true;
                continue;
            }
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                return Err(format!(
                    "line {}: metadata wants `key: value`, got {line:?}",
                    index + 1
                ));
            };
            match key.trim() {
                "viewport" => {
                    let value = value.trim();
                    let (w, h) = value
                        .split_once(['x', 'X'])
                        .ok_or_else(|| format!("line {}: viewport wants WxH", index + 1))?;
                    let (w, h) = (
                        w.trim()
                            .parse::<f32>()
                            .map_err(|e| format!("line {}: viewport width: {e}", index + 1))?,
                        h.trim()
                            .parse::<f32>()
                            .map_err(|e| format!("line {}: viewport height: {e}", index + 1))?,
                    );
                    viewport = Some(Size::new(w, h));
                }
                "mode" => {
                    mode = Some(match value.trim().to_lowercase().as_str() {
                        "zen" => emulator::Mode::Zen,
                        "patient" => emulator::Mode::Patient,
                        "immediate" => emulator::Mode::Immediate,
                        other => {
                            return Err(format!(
                                "line {}: mode {other:?} (want zen, patient or immediate)",
                                index + 1
                            ));
                        }
                    });
                }
                other => {
                    return Err(format!("line {}: unknown metadata key {other:?}", index + 1));
                }
            }
        } else {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            lines.push(line.to_owned());
        }
    }

    if !in_body {
        return Err("missing the `-----` metadata separator".into());
    }
    let viewport = viewport.ok_or("metadata is missing the viewport field")?;
    let mode = mode.ok_or("metadata is missing the mode field")?;
    Ok((viewport, mode, lines))
}

/// Runs one test on a freshly wiped sandbox. `Ok(())` means every
/// line executed (timed-out instructions included, they did run);
/// `Err` carries the failure already dumped into `errors_dir`.
fn run_test<P>(
    program: &P,
    options: &Options,
    test: &IceFile,
    errors_dir: &Path,
) -> Result<(), String>
where
    P: Program + 'static,
{
    let oryxis_dir = options.home.join(".oryxis");
    if oryxis_dir.exists() {
        std::fs::remove_dir_all(&oryxis_dir)
            .map_err(|e| format!("wiping {}: {e}", oryxis_dir.display()))?;
    }

    let per_test = Options {
        frontend: Frontend::Batch(test.path.clone()),
        home: options.home.clone(),
        shots: options.shots.clone(),
        viewport: test.viewport,
        scale: options.scale,
        mode: test.mode,
        timeout: options.timeout,
    };
    let (mut session, boot) = Session::new(program, &per_test);
    match boot {
        Pump::Ready | Pump::Timeout => {}
        Pump::Failed(instruction) => return Err(format!("boot failure: {instruction}")),
        Pump::Closed => return Err("emulator channel closed during boot".into()),
    }

    for (executed, line) in test.lines.iter().enumerate() {
        let (head, rest) = match line.split_once(char::is_whitespace) {
            Some((head, rest)) => (head, rest.trim()),
            None => (line.as_str(), ""),
        };
        let failure = match head {
            "settle" => {
                let idle = rest.parse::<u64>().unwrap_or(250).clamp(10, 5_000);
                session.settle(program, Duration::from_millis(idle), Duration::from_secs(30));
                None
            }
            "wait" => match rest.parse::<u64>() {
                Ok(ms) => {
                    session.wait(program, Duration::from_millis(ms.min(600_000)));
                    None
                }
                Err(_) => Some("wait wants milliseconds".to_owned()),
            },
            "timeout" => match rest.parse::<u64>() {
                Ok(ms) => {
                    session.timeout = Duration::from_millis(ms.clamp(100, 600_000));
                    None
                }
                Err(_) => Some("timeout wants milliseconds".to_owned()),
            },
            // Restart the app mid-test, the way the interactive front
            // ends do. `reset` keeps the sandbox vault, which is the
            // only way to assert what a SECOND session sees (a field
            // hydrated from storage, a setting that survived); `reset
            // wipe` returns to first-run inside a test that already
            // moved past it. The batch runner still wipes before every
            // test, so this is about the boundary WITHIN one.
            "reset" => {
                let wipe = rest == "wipe";
                if !rest.is_empty() && !wipe {
                    Some("reset takes nothing or `wipe`".to_owned())
                } else {
                    match session.reset(program, wipe) {
                        Ok(Pump::Ready | Pump::Timeout) => None,
                        Ok(Pump::Failed(instruction)) => {
                            Some(format!("reset: boot failure: {instruction}"))
                        }
                        Ok(Pump::Closed) => {
                            Some("reset: emulator channel closed".to_owned())
                        }
                        Err(reason) => Some(format!("reset: {reason}")),
                    }
                }
            }
            // The emulated desktop: which window is focused, and where
            // each one sits (see `Session::window_command`).
            "window" => session.window_command(program, false, rest).err(),
            "screenshot" => match session.screenshot(program, rest) {
                Ok((path, _png)) => {
                    println!("== shot {}", path.display());
                    None
                }
                Err(reason) => Some(format!("screenshot: {reason}")),
            },
            // Walk the widget tree without rendering. Two uses, and the
            // second one is why this is here at all:
            //
            // 1. `find "Text"` reports whether something is on screen, and
            //    how many times, which `expect` (exact match, pass/fail)
            //    cannot say.
            // 2. It REBUILDS THE TREE. The emulator only does that when
            //    something walks it, so a click by coordinate into a
            //    surface that just mounted (a panel, a hover-revealed
            //    action) lands on nothing until one does. `screenshot`
            //    also has that effect and used to be the only way to get
            //    it in a committed test, which meant paying for a PNG and
            //    leaving a CI artifact that documents nothing.
            //
            // A miss is NOT a failure here: nothing is asserted, and a
            // test that wants an assertion has `expect`.
            // The negative assertion `find` deliberately is not: a row
            // that should have disappeared (a conditional sub-option, a
            // closed panel) has no other way to be pinned in a committed
            // test, and `expect` can only say "this is here".
            "absent" => match session.texts(program) {
                Ok(entries) => {
                    match super::parse_quoted(rest) {
                        None => Some(format!("{head}: absent wants a quoted string")),
                        Some(needle) => {
                            let hits =
                                entries.iter().filter(|(t, _)| t.contains(&needle)).count();
                            if hits == 0 {
                                println!("== ok");
                                None
                            } else {
                                Some(format!("{head}: still on screen ({hits} matches)"))
                            }
                        }
                    }
                }
                Err(reason) => Some(format!("{head}: {reason}")),
            },
            "find" | "texts" => match session.texts(program) {
                Ok(entries) => {
                    let needle = super::parse_quoted(rest);
                    let count = match &needle {
                        Some(n) => entries.iter().filter(|(t, _)| t.contains(n)).count(),
                        None => entries.len(),
                    };
                    println!("== ok {count} {}", if needle.is_some() { "matches" } else { "texts" });
                    None
                }
                Err(reason) => Some(format!("{head}: {reason}")),
            },
            // Seed / report / assert the emulated clipboard. `clipboard is
            // "text"` is a real assertion, so copy paths (which land in the
            // emulated clipboard now that every access goes through the iced
            // runtime) are testable without a screenshot.
            "clipboard" => match session.clipboard_command(rest) {
                Ok(line) => {
                    if rest.is_empty() {
                        println!("== {line}");
                    }
                    None
                }
                Err(reason) => Some(reason),
            },
            _ => match session.run_line(program, line) {
                RunOutcome::Done | RunOutcome::Timeout => None,
                RunOutcome::Failed(instruction) => Some(format!("failed: {instruction}")),
                RunOutcome::Closed => Some("emulator channel closed".to_owned()),
                RunOutcome::Parse(error) => Some(format!("parse error: {error}")),
            },
        };

        if let Some(reason) = failure {
            dump_failure(program, &mut session, test, errors_dir, executed);
            return Err(format!("line {line:?}: {reason}"));
        }
    }
    Ok(())
}

/// Writes the failure evidence: a PNG of the UI at the moment of
/// failure and a reproduction `.ice` holding the `executed` lines
/// that ran before the failing one.
fn dump_failure<P>(
    program: &P,
    session: &mut Session<P>,
    test: &IceFile,
    errors_dir: &Path,
    executed: usize,
) where
    P: Program + 'static,
{
    use std::fmt::Write as _;

    if std::fs::create_dir_all(errors_dir).is_err() {
        return;
    }

    let stem = test
        .path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("test");
    if let Ok((_, png)) = session.screenshot(program, "") {
        let _ = std::fs::write(errors_dir.join(format!("{stem}.png")), png);
    }

    let mut reproduction = String::new();
    let _ = writeln!(
        reproduction,
        "viewport: {}x{}",
        test.viewport.width as u32, test.viewport.height as u32
    );
    let _ = writeln!(reproduction, "mode: {}", test.mode);
    let _ = writeln!(reproduction, "-----");
    for line in &test.lines[..executed] {
        let _ = writeln!(reproduction, "{line}");
    }
    let _ = std::fs::write(errors_dir.join(format!("{stem}.ice")), reproduction);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_header_body_and_comments() {
        let (viewport, mode, lines) = parse_ice(
            "viewport: 800x600\nmode: Patient\n-----\n# comment\n\nclick \"Hosts\"\nsettle 800\n",
        )
        .unwrap();
        assert_eq!(viewport, Size::new(800.0, 600.0));
        assert_eq!(mode, emulator::Mode::Patient);
        assert_eq!(lines, vec!["click \"Hosts\"", "settle 800"]);
    }

    #[test]
    fn header_is_required() {
        assert!(parse_ice("click \"Hosts\"\n").is_err());
        assert!(parse_ice("viewport: 800x600\n-----\n").is_err());
        assert!(parse_ice("mode: zen\n-----\n").is_err());
        assert!(parse_ice("viewport: 800x600\nmode: dance\n-----\n").is_err());
        assert!(parse_ice("viewport: 800\nmode: zen\n-----\n").is_err());
        assert!(parse_ice("viewport: 800x600\nspeed: fast\n-----\n").is_err());
    }
}
