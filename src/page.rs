//! The page: the release this machine runs, then whichever of these is
//! happening — looking for a newer one, the one found and its plan, or the
//! upgrade to it, with each question peipkg asks put as it asks it.

use libgxwi::escape;
use libgxwi::settings::{self, Glyph, Kind, More, Tile, Tone};

use crate::manager::{Check, Ended, Manager, Purpose, Run};
use crate::tool::{Operation, Plan, Progress, Question, Status};

const ABOUT: &str = "Move this machine to the next release of Peios: the release's packages first, then the settings it asks for.";

const DENIED: &str = "You can see which release this is, but you can't upgrade it: that needs write access to the package records and the release's settings. As shipped, only Administrators have it.";

pub fn page(m: &Manager) -> String {
    let mut page = settings::head(Glyph::Refresh, Tile::Violet, "Upgrade Peios", ABOUT);
    let status = match &m.status {
        None => return page + &settings::group("", &settings::row("Reading which release this is…", "", ""), ""),
        Some(Err(why)) => return page + &settings::group("", &settings::row("Which release this is can't be read", &sentence(why), ""), ""),
        Some(Ok(status)) => status,
    };
    if m.close_when_done && m.busy() {
        page.push_str(&settings::caution("Upgrade Peios closes when the upgrade is finished."));
    }
    if !status.may_upgrade {
        page.push_str(&settings::banner(DENIED));
    }
    page.push_str(&release(m, status));
    if let Some(run) = &m.run {
        page.push_str(&self::run(run, status));
        return page;
    }
    match &status.queued_seeds {
        Some(queued) if !queued.is_empty() => page.push_str(&waiting(queued, status.may_upgrade)),
        Some(_) => {}
        // Said, not left out: whether settings wait isn't known.
        None => page.push_str(&settings::hint("Whether any of this release's settings are waiting for the next restart can't be read.")),
    }
    if status.may_upgrade {
        page.push_str(&check(m, status));
    }
    page.push_str(&settings::hint("Software other than Peios itself is updated in Package Manager."));
    page
}

/// The release this machine runs, and how it stands.
fn release(m: &Manager, s: &Status) -> String {
    let name = s.name.clone().unwrap_or_else(|| s.edition.clone());
    let under = match &s.version {
        Some(v) => format!("{} {v}", s.edition),
        None => s.edition.clone(),
    };
    let pill = match &m.check {
        _ if !s.may_upgrade => String::new(),
        Check::Checking => settings::pill("Checking…", Tone::Plain),
        Check::Current => settings::pill("Up to Date", Tone::Good),
        Check::Available(plan) => settings::pill(&format!("{} Available", target(plan, s).unwrap_or_else(|| "A Release".into())), Tone::Warn),
        Check::Failed { .. } => settings::pill("Not Checked", Tone::Plain),
        Check::Unchecked => String::new(),
    };
    settings::hero(&settings::hero_title(Glyph::Screen, Tile::Blue, &name, &under), &pill)
}

/// The release the plan moves to, as its version: 2026.9.
fn target(plan: &Plan, s: &Status) -> Option<String> {
    let to = plan.operations.iter().find(|o| o.name == s.edition)?.to.as_deref()?;
    Some(to.rsplit_once('-').map_or(to, |(v, _)| v).to_string())
}

/// The release by its name: Peios 2026.9 Experimental.
fn named(version: &str, s: &Status) -> String {
    match &s.variant {
        Some(variant) => format!("Peios {version} {variant}"),
        None => format!("Peios {version}"),
    }
}

/// The release's settings waiting for the next boot.
fn waiting(queued: &[String], may: bool) -> String {
    let left = if queued.len() == 1 {
        "One of this release's settings was left to apply when the machine next starts.".to_string()
    } else {
        format!("{} of this release's settings were left to apply when the machine next starts.", queued.len())
    };
    // --seeds-only gathers and applies every setting the release names, so
    // the waiting ones are among what is applied, not all of it.
    let about = format!("{left} Apply Now applies all of the release's settings again, those among them, so they are in place at once.");
    let rows = settings::row(
        "Settings Waiting for the Next Restart",
        &about,
        &if may { settings::button("Apply Now", "apply-settings", &[], Kind::Plain, true) } else { String::new() },
    );
    settings::group("", &rows, "")
}

/// Looking for a newer release, and what was found.
fn check(m: &Manager, s: &Status) -> String {
    match &m.check {
        Check::Unchecked => settings::group(
            "",
            &settings::row("Is there a newer release?", "Looking refreshes the repositories this machine trusts.", &settings::button("Check Now", "check", &[], Kind::Primary, true)),
            "",
        ),
        Check::Checking => settings::group("", &format!(r#"<div class="st-row going">{}</div>"#, settings::progress(0, 0, "Looking for a newer release…")), ""),
        Check::Current => settings::group(
            "",
            &settings::row("Peios is up to date", "No newer release of this edition is offered.", &settings::button("Check Again", "check", &[], Kind::Plain, true)),
            "",
        ),
        Check::Failed { code, message } => {
            let (title, about, retry) = failure(code, message, !m.stale_ok);
            let button = match retry {
                Some((label, "--allow-stale")) => settings::button(label, "check-anyway", &[], Kind::Primary, true),
                _ => settings::button("Check Again", "check", &[], Kind::Plain, true),
            };
            settings::group("", &settings::row(&title, &about, &button), "")
        }
        Check::Available(plan) => available(plan, s),
    }
}

/// The release found: what upgrading changes, and the choice of when its
/// settings apply.
fn available(plan: &Plan, s: &Status) -> String {
    let version = target(plan, s).unwrap_or_else(|| "the next release".into());
    let mut out = changes(plan, &format!("{} Is Available", named(&version, s)));
    let when = settings::row(
        "Apply Its Settings at the Next Restart",
        "The release's settings — which services run, which policies apply — are applied as soon as it is installed. Leave them until the machine restarts instead when one would change something under the people signed in now.",
        &settings::switch("on-reboot", "Apply its settings at the next restart", true),
    );
    out.push_str(&settings::group("Its Settings", &when, ""));
    out.push_str(&settings::actions(&settings::button(&format!("Upgrade to {version}"), "upgrade", &[], Kind::Primary, true)));
    out
}

/// One change of the plan, as a row.
fn operation(op: &Operation) -> String {
    let (glyph, tile) = match op.kind.as_str() {
        "install" => (Glyph::Download, Tile::Blue),
        "remove" => (Glyph::Blocked, Tile::Red),
        "upgrade" => (Glyph::Refresh, Tile::Green),
        _ => (Glyph::Refresh, Tile::Orange),
    };
    let from = op.from.as_deref().unwrap_or_default();
    let to = op.to.as_deref().unwrap_or_default();
    let what = match op.kind.as_str() {
        "install" => format!("Install {to}"),
        "remove" => format!("Remove {from}"),
        "upgrade" => format!("{from} → {to}"),
        _ => format!("Back {from} → {to}"),
    };
    let short = op.name.rsplit('.').next().unwrap_or(&op.name);
    let size = if op.kind == "remove" { String::new() } else { size(op.size_installed) };
    let control = format!(r#"<span class="what">{}</span><span class="size">{}</span>"#, escape(&what), escape(&size));
    settings::item(&settings::icon(glyph, tile), short, &[(op.name.as_str(), true)], &control)
}

/// A plan's changes, under `title`, with what they cost.
fn changes(plan: &Plan, title: &str) -> String {
    let rows: String = plan.operations.iter().map(operation).collect();
    let download: i64 = plan.operations.iter().filter(|o| o.kind != "remove" && !o.local).map(|o| o.size_download).sum();
    let coming: i64 = plan.operations.iter().filter(|o| o.kind != "remove").map(|o| o.size_installed).sum();
    let count = if plan.operations.len() == 1 { "1 change".to_string() } else { format!("{} changes", plan.operations.len()) };
    let foot = settings::hint(&format!("{count}. Downloads {}, and takes {} once installed.", size(download), size(coming)));
    let mut out = settings::group(title, &rows, &foot);
    for note in &plan.notes {
        out.push_str(&settings::caution(&sentence(note)));
    }
    out
}

/// A size in bytes, in the unit that suits it.
fn size(bytes: i64) -> String {
    let bytes = bytes.max(0) as f64;
    for (unit, scale) in [("GB", 1e9), ("MB", 1e6), ("KB", 1e3)] {
        if bytes >= scale {
            let n = bytes / scale;
            return if n < 10.0 { format!("{n:.1} {unit}") } else { format!("{n:.0} {unit}") };
        }
    }
    format!("{} bytes", bytes as i64)
}

/// A button the keyboard goes to as it appears, sending `values`.
fn focused(label: &str, event: &str, values: &[(&str, &str)], kind: Kind) -> String {
    settings::button(label, event, values, kind, true).replacen("<button ", "<button fx-autofocus ", 1)
}

/// The open question, with its answers.
fn question(q: &Question) -> String {
    let id = q.id.to_string();
    let values = [("id", id.as_str())];
    let (title, body) = match q.kind.as_str() {
        "authorise" => (
            "Needs Your Approval",
            format!(
                "<p>{}</p><p class=\"quiet\">This is allowed only when you say so, and your approval is recorded.</p>{}",
                escape(&sentence(&q.text)),
                settings::actions(&format!("{}{}", focused("Cancel", "refuse", &values, Kind::Plain), settings::button("Allow This", "allow", &values, Kind::Primary, true)))
            ),
        ),
        "modified" => (
            "A Changed File",
            format!(
                "<p><code class=\"st-mono\">{}</code> has been changed since it was installed, and the upgrade would remove it.</p><p class=\"quiet\">Keep it, and it stays as it is, belonging to no package. Delete it, and a copy is kept beside it.</p>{}",
                escape(&q.path),
                settings::actions(&format!(
                    "{}{}{}",
                    settings::button("Stop Everything", "modified", &[("id", &id), ("answer", "abort")], Kind::Quiet, true),
                    settings::button("Delete It", "modified", &[("id", &id), ("answer", "remove")], Kind::Danger, true),
                    focused("Keep It", "modified", &[("id", &id), ("answer", "keep")], Kind::Primary),
                ))
            ),
        ),
        // The plan isn't what the check found, so it is asked again.
        _ => (
            "This Differs From What Was Found",
            format!(
                "<p>What upgrading would change now isn't what the check found a moment ago: it is shown above. Upgrade with these changes?</p>{}",
                settings::actions(&format!("{}{}", focused("Cancel", "refuse", &values, Kind::Plain), settings::button("Upgrade", "proceed", &values, Kind::Primary, true)))
            ),
        ),
    };
    settings::group(title, &settings::more(More::Asking, &body), "")
}

/// What a step is doing, in words.
fn doing(p: &Progress) -> String {
    let name = p.package.as_deref().map(|n| n.rsplit('.').next().unwrap_or(n)).unwrap_or_default();
    let of = if p.steps > 1 { format!(" ({} of {})", p.step, p.steps) } else { String::new() };
    match p.phase.as_str() {
        "fetch" => format!("Downloading and checking {name}{of}…"),
        "stage" => format!("Preparing {name}{of}…"),
        "apply" => "Putting the new release in place…".into(),
        "commit" => "Recording the changes…".into(),
        "release-stage" => "Gathering the release's settings…".into(),
        "release-apply" => "Applying the release's settings…".into(),
        _ => "Finishing up…".into(),
    }
}

/// How far along a run is, as a bar: two steps a package coming in, three
/// for the whole, then the release's own one or two.
fn fraction(run: &Run) -> (usize, usize) {
    let coming = run.plan.as_ref().map_or(0, |p| p.operations.iter().filter(|o| o.kind != "remove").count());
    let packages = if run.purpose == Purpose::Upgrade { coming * 2 + 3 } else { 0 };
    let queued = run.args.iter().any(|a| a == "--on-reboot");
    let total = packages + if queued { 1 } else { 2 };
    let Some(p) = &run.progress else { return (0, total) };
    let done = match p.phase.as_str() {
        "fetch" => p.step.saturating_sub(1),
        "stage" => coming + p.step.saturating_sub(1),
        "apply" => coming * 2,
        "commit" => coming * 2 + 1,
        "release-stage" => packages,
        "release-apply" => packages + 1,
        _ => coming * 2 + 2,
    };
    (done, total)
}

/// What a failure means, and the way on where there is one.
fn failure(code: &str, message: &str, may_retry: bool) -> (String, String, Option<(&'static str, &'static str)>) {
    let (title, about, retry) = match code {
        "stale" => (
            "A repository's information is out of date",
            "Refreshing it didn't bring anything newer. You can carry on with what this machine already has, which may lack recent releases and fixes. Carrying on is recorded.",
            Some(("Continue Anyway", "--allow-stale")),
        ),
        "busy" => ("Software is being changed", "Another installation or removal is under way. Try again once it has finished.", Some(("Try Again", ""))),
        "denied" => ("You can't upgrade Peios", "It needs write access to the package records and the release's settings. As shipped, only Administrators have it.", None),
        "untrusted" => (
            "A repository isn't trusted yet",
            "It is set up on this machine, but its trust hasn't been confirmed, so nothing can be upgraded until it is. Confirm it, or remove it, under Repositories in Package Manager.",
            None,
        ),
        "unresolvable" => ("The upgrade can't be made", "What the new release needs can't all be had together.", None),
        "no-release" => ("Which release this is can't be told", "", None),
        "seeds" => ("The release's settings couldn't be gathered", "The new release is installed, but a setting it asks for isn't there. Applying its settings again tries once more.", None),
        "apply" => ("The release's settings couldn't all be applied", "The new release is installed; its settings are left waiting for the next restart, which tries them again.", None),
        _ => ("The upgrade failed", "", None),
    };
    let retry = if may_retry { retry } else { None };
    let mut about = about.to_string();
    let detail = detail(message);
    if !detail.is_empty() {
        if !about.is_empty() {
            about.push(' ');
        }
        about.push_str(&sentence(detail));
    }
    (title.to_string(), about, retry)
}

/// The first line of what a tool said on failing, without where it came
/// from in front ("upgrade: ", "peipkg/resolver: "), which is for its log
/// and not for a person.
fn detail(message: &str) -> &str {
    let mut detail = message.lines().next().unwrap_or_default().trim();
    while let Some((head, rest)) = detail.split_once(": ")
        && head.len() <= 24
        && head.chars().all(|c| c.is_ascii_lowercase() || matches!(c, ' ' | '/' | '-'))
    {
        detail = rest;
    }
    detail
}

/// The run, after the release.
fn run(run: &Run, s: &Status) -> String {
    let mut out = String::new();
    let failed = matches!(run.end, Some(Ended::Failed { .. }));
    if let Some(plan) = run.plan.as_ref().filter(|p| !p.operations.is_empty() && !failed) {
        let version = target(plan, s).unwrap_or_else(|| "the next release".into());
        out.push_str(&changes(plan, &format!("Upgrading to {}", named(&version, s))));
    }
    match (&run.end, &run.question) {
        (None, Some(q)) => out.push_str(&question(q)),
        (None, None) => {
            let (done, of) = fraction(run);
            let text = match &run.progress {
                Some(p) => doing(p),
                None if run.purpose == Purpose::Settings => "Applying the release's settings…".into(),
                None => "Working out what's needed…".into(),
            };
            let bar = if run.progress.is_some() { settings::progress(done, of, &text) } else { settings::progress(0, 0, &text) };
            out.push_str(&settings::group("", &format!(r#"<div class="st-row going">{bar}</div>"#), ""));
        }
        (Some(Ended::Done { queued }), _) => {
            let (big, under) = match (run.purpose, queued) {
                (Purpose::Settings, _) => ("Applied".to_string(), "This release's settings are in place.".to_string()),
                (Purpose::Upgrade, true) => (
                    "Upgraded".to_string(),
                    "The new release is installed. Its settings apply when the machine next restarts, which also starts its new kernel, if it has one.".to_string(),
                ),
                (Purpose::Upgrade, false) => (
                    "Upgraded".to_string(),
                    "The new release is installed and its settings are applied. Restarting starts its new kernel, if it has one.".to_string(),
                ),
            };
            out.push_str(&settings::hero(&settings::big(&big, &under), &settings::pill("Finished", Tone::Good)));
            out.push_str(&settings::actions(&settings::focused_button("Close", "dismiss", Kind::Primary)));
        }
        (Some(Ended::Cancelled), _) => {
            out.push_str(&settings::hero(&settings::big("Cancelled", "Nothing was changed."), ""));
            out.push_str(&settings::actions(&settings::focused_button("Close", "dismiss", Kind::Primary)));
        }
        (Some(Ended::Failed { code, message }), _) => {
            let flagged = run.args.iter().any(|a| a == "--allow-stale");
            let (title, about, retry) = failure(code, message, !flagged || code == "busy");
            out.push_str(&settings::group("", &settings::row(&title, &about, ""), ""));
            let retry = match retry {
                Some((label, flag)) => settings::button(label, "retry", &[("flag", flag)], Kind::Primary, true),
                None => String::new(),
            };
            out.push_str(&settings::actions(&format!("{}{retry}", settings::focused_button("Close", "dismiss", Kind::Plain))));
        }
    }
    out.push_str(&notes(&run.warnings));
    out
}

/// What peipkg warned of, each once.
fn notes(warnings: &[String]) -> String {
    let mut seen: Vec<String> = Vec::new();
    for w in warnings {
        let w = sentence(w);
        if !seen.contains(&w) {
            seen.push(w);
        }
    }
    if seen.is_empty() {
        return String::new();
    }
    let rows: String = seen.iter().map(|w| format!(r#"<div class="st-row note"><span class="label"><small>{}</small></span></div>"#, escape(w))).collect();
    settings::group(if seen.len() == 1 { "Note" } else { "Notes" }, &rows, "")
}

/// Words as a sentence: a capital first letter and a full stop.
pub fn sentence(text: &str) -> String {
    let text = text.trim();
    let mut chars = text.chars();
    let Some(first) = chars.next() else { return String::new() };
    let mut out: String = first.to_uppercase().chain(chars).collect();
    if !out.ends_with(['.', '!', '?']) {
        out.push('.');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> Status {
        Status { edition: "dev.peios.peios-experimental".into(), variant: Some("Experimental".into()), may_upgrade: true, ..Default::default() }
    }

    #[test]
    fn the_target_is_the_editions_new_version() {
        let plan = Plan {
            operations: vec![
                Operation { kind: "upgrade".into(), name: "dev.peios.net".into(), to: Some("0.1.7-1".into()), ..Default::default() },
                Operation { kind: "upgrade".into(), name: "dev.peios.peios-experimental".into(), to: Some("2026.9-1".into()), ..Default::default() },
            ],
            ..Default::default()
        };
        assert_eq!(target(&plan, &status()).as_deref(), Some("2026.9"));
        assert_eq!(named("2026.9", &status()), "Peios 2026.9 Experimental");
    }

    #[test]
    fn a_stale_refusal_offers_carrying_on_once() {
        assert_eq!(failure("stale", "", true).2, Some(("Continue Anyway", "--allow-stale")));
        assert_eq!(failure("stale", "", false).2, None);
        assert!(failure("untrusted", "", true).1.contains("Package Manager"));
        assert!(failure("stale", "upgrade: repository \"m\" serves stale metadata", true).1.ends_with(" Repository \"m\" serves stale metadata."));
    }
}
