//! The window: which release this machine runs, whether a newer one is
//! offered, and the upgrade to it.
//!
//! Everything is read from upgrade-peios as the person, and an upgrade is
//! upgrade-peios's driven run, made as the person: Upgrade Peios has no
//! authority of its own. Whether the person may upgrade is asked of the
//! system, by upgrade-peios; where they may not, the release is shown as
//! it is, with the reason said once.

use std::sync::Weak;

use libgxwi::settings;
use libgxwi::{Facts, Fields, Live, Surface, Value};

use crate::page;
use crate::tool::{self, Answers, Event, Plan, Progress, Question, Status};

/// What the check for a newer release found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// Not looked yet, or not to be: the person may not upgrade.
    Unchecked,
    Checking,
    Current,
    /// A newer release, and the plan upgrading to it would make.
    Available(Plan),
    Failed { code: String, message: String },
}

/// What a run is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// The release, then its settings.
    Upgrade,
    /// Only the installed release's settings, again.
    Settings,
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// Its settings were queued for the next boot rather than applied.
    Done { queued: bool },
    Cancelled,
    Failed { code: String, message: String },
}

/// An upgrade being made, or the last one, until it is put away.
pub struct Run {
    /// Which run this is, so a run's late events can't land on the next.
    pub number: u64,
    pub purpose: Purpose,
    /// What upgrade-peios was run with, and what a retry added.
    pub args: Vec<String>,
    /// The plan the person was shown before choosing to upgrade: when the
    /// run's own plan is the same, its proceed is theirs already.
    pub shown: Option<Plan>,
    pub plan: Option<Plan>,
    pub question: Option<Question>,
    pub progress: Option<Progress>,
    pub warnings: Vec<String>,
    /// What was said last, while it works.
    pub doing: Option<String>,
    pub end: Option<Ended>,
    pub answers: Option<Answers>,
    pub proceeded: bool,
}

impl Run {
    pub fn going(&self) -> bool {
        self.end.is_none()
    }

    /// Past the point of being called off: the plan is approved, or there
    /// was none to approve.
    pub fn committed(&self) -> bool {
        self.going() && (self.proceeded || self.purpose == Purpose::Settings)
    }

    fn heard(&mut self, event: Event) {
        match event {
            Event::Plan(plan) => self.plan = Some(plan),
            Event::Question(question) => {
                // Shown and chosen already: the same plan needs no second yes.
                let same = self.shown.as_ref().is_some_and(|shown| self.plan.as_ref().is_some_and(|plan| plan.operations == shown.operations));
                if question.kind == "proceed" && same {
                    self.answer(question.id, "yes");
                    return;
                }
                self.question = Some(question);
            }
            Event::Progress(progress) => {
                self.question = None;
                self.progress = Some(progress);
            }
            Event::Warning { text } => self.warnings.push(text),
            Event::Message { text } => self.doing = Some(text),
            Event::Done { seeds_queued, .. } => {
                self.question = None;
                self.end = Some(Ended::Done { queued: seeds_queued });
            }
            Event::Cancelled { .. } => {
                self.question = None;
                self.end = Some(Ended::Cancelled);
            }
            Event::Error { code, message } => {
                self.question = None;
                self.end = Some(Ended::Failed { code, message });
            }
        }
    }

    /// Answers the open question.
    pub fn answer(&mut self, id: u64, answer: &str) {
        if let Some(answers) = &self.answers {
            answers.answer(id, answer);
        }
        if answer == "yes" && self.question.as_ref().is_none_or(|q| q.kind == "proceed" && q.id == id) {
            self.proceeded = true;
        }
        self.question = None;
    }
}

pub struct Manager {
    pub window: Weak<Surface<Manager>>,
    /// The installed release, once read.
    pub status: Option<Result<Status, String>>,
    pub check: Check,
    /// The person said to carry on past a repository's out-of-date
    /// information, for as long as the window is open.
    pub stale_ok: bool,
    pub run: Option<Run>,
    runs: u64,
    /// The window was asked to close while an upgrade was being made.
    pub close_when_done: bool,
    said: Option<Result<String, String>>,
}

impl Manager {
    pub fn new() -> Manager {
        Manager { window: Weak::new(), status: None, check: Check::Unchecked, stale_ok: false, run: None, runs: 0, close_when_done: false, said: None }
    }

    pub fn busy(&self) -> bool {
        self.run.as_ref().is_some_and(Run::going)
    }

    pub fn may(&self) -> bool {
        matches!(&self.status, Some(Ok(s)) if s.may_upgrade)
    }

    /// Reads the installed release again, on a thread; once read, looks
    /// for a newer one if `check` and the person may upgrade.
    pub fn reread(&self, check: bool) {
        let window = self.window.clone();
        std::thread::spawn(move || {
            let read = tool::status();
            if let Some(window) = window.upgrade() {
                window.update(|manager, _| {
                    manager.status = Some(read);
                    if check && manager.may() {
                        manager.look(false);
                    }
                });
            }
        });
    }

    /// Looks for a newer release, on a thread: a refresh of every trusted
    /// repository, then the plan an upgrade would make, without making it.
    pub fn look(&mut self, anyway: bool) {
        if self.check == Check::Checking || self.busy() {
            return;
        }
        self.stale_ok |= anyway;
        let stale_ok = self.stale_ok;
        self.check = Check::Checking;
        self.said = None;
        let window = self.window.clone();
        std::thread::spawn(move || {
            tool::refresh_trusted();
            let mut args = vec!["--check".to_string()];
            if stale_ok {
                args.push("--allow-stale".into());
            }
            let found = collect(&args);
            if let Some(window) = window.upgrade() {
                window.update(|manager, _| manager.check = found);
            }
        });
    }

    /// Starts a run of upgrade-peios with `args`.
    fn start(&mut self, purpose: Purpose, mut args: Vec<String>, shown: Option<Plan>) {
        if self.busy() || !self.may() {
            return;
        }
        if self.stale_ok && purpose == Purpose::Upgrade && !args.iter().any(|a| a == "--allow-stale") {
            args.push("--allow-stale".into());
        }
        self.runs += 1;
        let number = self.runs;
        let mut run = Run {
            number,
            purpose,
            args: args.clone(),
            shown,
            plan: None,
            question: None,
            progress: None,
            warnings: Vec::new(),
            doing: None,
            end: None,
            answers: None,
            proceeded: false,
        };
        let window = self.window.clone();
        // The window's lock is held here, so the first event waits for the
        // run to be in place before it lands.
        match tool::drive(&args, move |event| {
            if let Some(window) = window.upgrade() {
                window.update(|manager, _| manager.heard(number, event));
            }
        }) {
            Ok(answers) => run.answers = Some(answers),
            Err(message) => run.end = Some(Ended::Failed { code: "failed".into(), message }),
        }
        self.run = Some(run);
        self.said = None;
    }

    fn heard(&mut self, number: u64, event: Event) {
        let Some(run) = self.run.as_mut().filter(|r| r.number == number) else { return };
        let ended = event.ends();
        run.heard(event);
        if !ended {
            return;
        }
        if self.close_when_done
            && let Some(window) = self.window.upgrade()
        {
            window.close();
        }
        // What is installed, and what is offered, may differ now.
        if matches!(run.end, Some(Ended::Done { .. })) {
            self.check = Check::Unchecked;
            self.reread(true);
        }
    }

    /// Runs again what failed, with the flag that lets it through.
    fn retry(&mut self, flag: &str) {
        let Some(run) = self.run.take() else { return };
        let mut args = run.args.clone();
        self.stale_ok |= flag == "--allow-stale";
        if !flag.is_empty() && !args.iter().any(|a| a == flag) {
            args.push(flag.to_string());
        }
        self.start(run.purpose, args, run.shown);
    }
}

/// Runs a driven check to its end, and gives what it found.
fn collect(args: &[String]) -> Check {
    let (send, receive) = std::sync::mpsc::channel();
    let answers = match tool::drive(args, move |event| {
        let _ = send.send(event);
    }) {
        Ok(answers) => answers,
        Err(message) => return Check::Failed { code: "failed".into(), message },
    };
    answers.close();
    let mut plan = None;
    for event in receive {
        match event {
            Event::Plan(p) => plan = Some(p),
            Event::Done { .. } => {
                return match plan {
                    Some(p) if !p.operations.is_empty() => Check::Available(p),
                    _ => Check::Current,
                };
            }
            Event::Error { code, message } => return Check::Failed { code, message },
            Event::Cancelled { .. } => return Check::Unchecked,
            _ => {}
        }
    }
    Check::Failed { code: "failed".into(), message: "upgrade-peios stopped without saying how it ended".into() }
}

impl Live for Manager {
    fn render(&self, _facts: &Facts) -> String {
        settings::single(&page::page(self), &settings::status(self.said.as_ref(), ""))
    }

    fn event(&mut self, name: &str, value: &Value, fields: &mut Fields) {
        let id = || value.get("id").and_then(Value::as_str).and_then(|id| id.parse().ok()).unwrap_or(0);
        match name {
            "check" => self.look(false),
            "check-anyway" => {
                self.check = Check::Unchecked;
                self.look(true);
            }
            "upgrade" => {
                let Check::Available(plan) = &self.check else { return };
                let plan = plan.clone();
                let mut args = Vec::new();
                if fields.get("on-reboot") == "on" {
                    args.push("--on-reboot".to_string());
                }
                self.start(Purpose::Upgrade, args, Some(plan));
            }
            "apply-settings" => self.start(Purpose::Settings, vec!["--seeds-only".into()], None),
            "allow" | "proceed" => {
                if let Some(run) = &mut self.run {
                    run.answer(id(), "yes");
                }
            }
            "refuse" => {
                if let Some(run) = &mut self.run {
                    run.answer(id(), "no");
                }
            }
            "modified" => {
                let answer = value.get("answer").and_then(Value::as_str).unwrap_or("abort").to_string();
                if let Some(run) = &mut self.run {
                    run.answer(id(), &answer);
                }
            }
            "retry" => {
                let flag = value.get("flag").and_then(Value::as_str).unwrap_or_default().to_string();
                self.retry(&flag);
            }
            "dismiss" if !self.busy() => self.run = None,
            _ => {}
        }
    }

    fn closing(&mut self, _fields: &mut Fields) -> bool {
        // An upgrade past its approval finishes whatever happens; the window
        // stays to show how it ended, then goes.
        if self.run.as_ref().is_some_and(Run::committed) {
            self.close_when_done = true;
            return false;
        }
        if let Some(answers) = self.run.as_ref().and_then(|r| r.answers.as_ref()) {
            answers.close();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Operation;

    fn plan(to: &str) -> Plan {
        Plan { operations: vec![Operation { kind: "upgrade".into(), name: "e".into(), to: Some(to.into()), ..Default::default() }], ..Default::default() }
    }

    fn run(shown: Option<Plan>) -> Run {
        Run {
            number: 1,
            purpose: Purpose::Upgrade,
            args: vec![],
            shown,
            plan: None,
            question: None,
            progress: None,
            warnings: vec![],
            doing: None,
            end: None,
            answers: None,
            proceeded: false,
        }
    }

    #[test]
    fn the_plan_shown_is_approved_once_and_a_different_one_is_asked() {
        let mut same = run(Some(plan("2")));
        same.heard(Event::Plan(plan("2")));
        same.heard(Event::Question(Question { id: 1, kind: "proceed".into(), ..Default::default() }));
        assert!(same.proceeded && same.question.is_none());

        let mut other = run(Some(plan("2")));
        other.heard(Event::Plan(plan("3")));
        other.heard(Event::Question(Question { id: 1, kind: "proceed".into(), ..Default::default() }));
        assert!(!other.proceeded && other.question.is_some());
    }

    #[test]
    fn an_authorisation_is_always_asked() {
        let mut r = run(Some(plan("2")));
        r.heard(Event::Plan(plan("2")));
        r.heard(Event::Question(Question { id: 1, kind: "authorise".into(), ..Default::default() }));
        assert!(r.question.is_some() && !r.proceeded);
    }
}
