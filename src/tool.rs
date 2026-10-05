//! Running upgrade-peios, the only thing that moves this machine to the
//! next release. `--status --json` says which release this is; `--driven`
//! is a conversation in JSON Lines: peipkg's plan, each of its questions
//! and its progress, then the release's own progress and how it ended
//! (Upgrading Peios, for a program). Upgrade Peios never parses the words
//! either tool prints for a person.

use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde::Deserialize;

/// Where upgrade-peios is.
pub const PROGRAM: &str = "/usr/bin/upgrade-peios";
/// Where peipkg is: asked only which repositories are trusted, and to
/// refresh those.
pub const PEIPKG: &str = "/usr/bin/peipkg";

/// The installed release, as `upgrade-peios --status --json` gives it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Status {
    /// The edition package: dev.peios.peios-experimental.
    pub edition: String,
    /// Peios 2026.8 Experimental.
    pub name: Option<String>,
    pub version_id: Option<String>,
    pub variant: Option<String>,
    /// The edition package's version, where peipkg's records can be read.
    pub version: Option<String>,
    /// The release's seeds waiting for the next boot; `None` where the
    /// queue can't be read.
    pub queued_seeds: Option<Vec<String>>,
    /// The person may upgrade, asked of the system by upgrade-peios.
    pub may_upgrade: bool,
}

/// Which release this is.
pub fn status() -> Result<Status, String> {
    let out = Command::new(PROGRAM)
        .args(["--status", "--json"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("upgrade-peios couldn't be run: {e}"))?;
    if !out.status.success() {
        return Err(said(&String::from_utf8_lossy(&out.stderr), "upgrade-peios"));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("upgrade-peios's answer couldn't be read: {e}"))
}

/// The last thing a tool said on failing, without its name in front.
fn said(stderr: &str, tool: &str) -> String {
    let line = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("it failed without saying why");
    line.trim().trim_start_matches(&format!("{tool}: ")).to_string()
}

/// One event of a driven run: peipkg's, passed on, and upgrade-peios's own.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum Event {
    Plan(Plan),
    Question(Question),
    Progress(Progress),
    Warning {
        text: String,
    },
    Message {
        text: String,
    },
    Done {
        #[serde(default)]
        summary: Option<String>,
        /// The release's seeds were queued for the next boot, not applied.
        #[serde(default)]
        seeds_queued: bool,
    },
    Cancelled {
        #[serde(default)]
        reason: String,
    },
    Error {
        code: String,
        message: String,
    },
}

impl Event {
    /// Whether this is how the run ended.
    pub fn ends(&self) -> bool {
        matches!(self, Event::Done { .. } | Event::Cancelled { .. } | Event::Error { .. })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Plan {
    pub operations: Vec<Operation>,
    pub authorisations: Vec<String>,
    pub notes: Vec<String>,
    pub other_roots: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Operation {
    /// install, upgrade, downgrade or remove.
    pub kind: String,
    pub name: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub repository: Option<String>,
    pub local: bool,
    pub size_download: i64,
    pub size_installed: i64,
    pub root: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Question {
    pub id: u64,
    /// authorise, proceed or modified.
    pub kind: String,
    pub text: String,
    pub package: String,
    pub path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Progress {
    /// peipkg's fetch, stage, apply, commit or finish, then upgrade-peios's
    /// release-stage and release-apply.
    pub phase: String,
    pub step: usize,
    pub steps: usize,
    pub package: Option<String>,
}

/// Where a driven run's answers go: upgrade-peios hands its input to
/// peipkg, so these reach peipkg under its own question ids. Dropping every
/// copy closes it, which refuses whatever is asked next.
#[derive(Clone)]
pub struct Answers(Arc<Mutex<Option<ChildStdin>>>);

impl Answers {
    /// Answers question `id`.
    pub fn answer(&self, id: u64, answer: &str) {
        let line = serde_json::json!({ "id": id, "answer": answer }).to_string() + "\n";
        if let Some(stdin) = self.0.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            let _ = stdin.write_all(line.as_bytes()).and_then(|()| stdin.flush());
        }
    }

    /// Closes the input: whatever is asked next is refused.
    pub fn close(&self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
}

/// Starts `program --driven` with `args`, handing each event to `heard` on
/// a thread of its own, ending with exactly one that [`Event::ends`] it.
fn drive_program(program: &'static str, args: &[String], mut heard: impl FnMut(Event) + Send + 'static) -> Result<Answers, String> {
    let name = program.rsplit('/').next().unwrap_or(program);
    let mut child = Command::new(program)
        .arg("--driven")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{name} couldn't be run: {e}"))?;
    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let answers = Answers(Arc::new(Mutex::new(child.stdin.take())));
    let closing = answers.clone();
    std::thread::spawn(move || {
        let mut ended = false;
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let Ok(event) = serde_json::from_str::<Event>(&line) else { continue };
            ended = event.ends();
            heard(event);
            if ended {
                break;
            }
        }
        closing.close();
        // Anything written outside the events is a fault: a panic, say. It
        // is the best word on a run that ended unsaid.
        let mut fault = String::new();
        let _ = std::io::Read::read_to_string(&mut BufReader::new(stderr), &mut fault);
        let _ = child.wait();
        if !ended {
            let message = if fault.trim().is_empty() { format!("{name} stopped without saying how it ended") } else { said(&fault, name) };
            heard(Event::Error { code: "failed".into(), message });
        }
    });
    Ok(answers)
}

/// Starts `upgrade-peios --driven` with `args`.
pub fn drive(args: &[String], heard: impl FnMut(Event) + Send + 'static) -> Result<Answers, String> {
    drive_program(PROGRAM, args, heard)
}

/// A configured repository, as `peipkg repo list --json` gives it: only
/// what deciding to refresh it needs.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Repository {
    name: String,
    trusted: bool,
}

/// Refreshes every trusted repository, by name, and waits for it. With no
/// names, `peipkg refresh` would also run the trust ceremony of a
/// repository set up but never confirmed; trusting one is the person's
/// choice, in Package Manager, never a side effect of looking for a
/// release. Whatever goes wrong is left for the check after it to say.
pub fn refresh_trusted() {
    let Ok(out) = Command::new(PEIPKG).args(["repo", "list", "--json"]).stdin(Stdio::null()).output() else { return };
    let repositories: Vec<Repository> = serde_json::from_slice(&out.stdout).unwrap_or_default();
    let mut args = vec!["refresh".to_string()];
    args.extend(repositories.into_iter().filter(|r| r.trusted).map(|r| r.name));
    if args.len() == 1 {
        return;
    }
    let (send, receive) = std::sync::mpsc::channel();
    let Ok(answers) = drive_program(PEIPKG, &args, move |event| {
        let _ = send.send(event.ends());
    }) else {
        return;
    };
    answers.close();
    for ended in receive {
        if ended {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_reads_as_upgrade_peios_writes_it() {
        let status: Status = serde_json::from_str(
            r#"{"edition":"dev.peios.peios-experimental","may_upgrade":true,"name":"Peios 2026.8 Experimental","queued_seeds":[],"variant":"Experimental","version":"2026.8-1","version_id":"2026.8"}"#,
        )
        .unwrap();
        assert_eq!(status.version.as_deref(), Some("2026.8-1"));
        assert_eq!(status.queued_seeds, Some(vec![]));
        let unread: Status = serde_json::from_str(r#"{"edition":"e","may_upgrade":false,"queued_seeds":null,"version":null}"#).unwrap();
        assert_eq!((unread.queued_seeds, unread.may_upgrade), (None, false));
    }

    #[test]
    fn its_own_events_read_beside_peipkgs() {
        let done: Event = serde_json::from_str(r#"{"edition":"e","event":"done","seeds_queued":true,"summary":"1 seed(s) queued"}"#).unwrap();
        assert_eq!(done, Event::Done { summary: Some("1 seed(s) queued".into()), seeds_queued: true });
        let checked: Event = serde_json::from_str(r#"{"edition":"e","event":"done","summary":"dry run"}"#).unwrap();
        assert!(matches!(checked, Event::Done { seeds_queued: false, .. }));
        let staging: Event = serde_json::from_str(r#"{"event":"progress","phase":"release-stage","step":1,"steps":2}"#).unwrap();
        assert!(matches!(staging, Event::Progress(Progress { ref phase, .. }) if phase == "release-stage"));
    }

    #[test]
    fn a_failure_is_its_last_line() {
        assert_eq!(said("upgrade-peios: no VARIANT_ID\n", "upgrade-peios"), "no VARIANT_ID");
    }
}
