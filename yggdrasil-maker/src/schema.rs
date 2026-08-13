//! The widget schema — what yggterm paints.
//!
//! ⭐ THIS FILE IS THE REMAKE. The maker used to declare a `web-surface`: a
//! WebKitGTK child webview, with its own HTML, CSS and JS, to draw a form and
//! three lists. That is Tier B in the app-architecture spec, and the spec is
//! blunt about what Tier B is — **a COST, never a choice**. It buys a foreign
//! engine and pays for it forever: no faithful screenshot into the surface, no
//! `dom-eval`, no inherited theme, its own compositing and its own lifecycle,
//! plus two web processes per surface.
//!
//! Nothing here needs an engine. A checkout survey, a plan and a build log are
//! lists, labels and a couple of buttons — which is the definition of Tier A.
//! So the app now declares widgets and the host paints them as ordinary shell
//! DOM: themed with the terminal, screenshot-faithful by construction, and
//! drivable by the same automation as everything else.
//!
//! ⛔ NOTHING HERE IS A NEW WIDGET KIND. Every one already existed, which is the
//! point of the tier: `section` (with `card`), `label`, `search-box`, `tabs`,
//! `toggle`, `button`, `list-row` (with `status` and `actions`), `markdown`.
//!
//! ⛔ AND NO CONTEXT MENU. `list-row` offers one; this app declines it, by
//! requirement. Every verb is a visible button on the row it acts on.

use serde_json::{json, Value};

pub const TAB_REPO: &str = "repo";
pub const TAB_PLAN: &str = "plan";
pub const TAB_BUILD: &str = "build";

/// What the user has chosen and what the pane is showing.
///
/// ⛔ THE APP OWNS THIS, NOT THE PANE. The host echoes a widget's draft back on
/// an action and nothing more; if the selection lived only in the rendered
/// schema it would reset every time a refresh landed mid-decision.
pub struct View {
    pub tab: String,
    pub config: String,
    pub profile: String,
    pub skip_smoke: bool,
    pub filter: String,
    /// The last action's outcome, in the words of whatever performed it.
    pub notice: Option<String>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            tab: TAB_REPO.to_string(),
            config: String::new(),
            profile: String::new(),
            skip_smoke: false,
            filter: String::new(),
            notice: None,
        }
    }
}

fn label(text: impl Into<String>) -> Value {
    json!({"kind": "label", "text": text.into()})
}

/// ⛔ THE FIELD IS `text`, NOT `title` — a `section` names its heading the way a
/// `label` does, while a `list-row` uses `title`. Guessing wrong fails the WHOLE
/// pane at render with `missing field 'text'`, which is the loud version of this
/// mistake. The quiet version is worse: `tabs` selects with `active`, and
/// `selected` is silently defaulted, so the pane renders with no tab highlighted
/// and nothing anywhere says why. Both were paid for in the sibling fleet app.
fn section(text: impl Into<String>, card: bool) -> Value {
    json!({"kind": "section", "text": text.into(), "card": card})
}

fn matches(filter: &str, haystack: &[&str]) -> bool {
    if filter.trim().is_empty() {
        return true;
    }
    let needle = filter.to_lowercase();
    haystack.iter().any(|h| h.to_lowercase().contains(&needle))
}

fn size(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
    } else if bytes >= 1 << 20 {
        format!("{} MB", bytes >> 20)
    } else {
        format!("{} KB", bytes >> 10)
    }
}

fn header(view: &View) -> Vec<Value> {
    let mut widgets = vec![json!({
        "kind": "tabs",
        "id": "tab",
        "action": "tab",
        "active": view.tab,
        "tabs": [
            {"id": TAB_REPO, "label": "Checkout"},
            {"id": TAB_PLAN, "label": "Plan"},
            {"id": TAB_BUILD, "label": "Build"},
        ],
    })];
    if let Some(notice) = &view.notice {
        widgets.push(label(notice.clone()));
    }
    widgets
}

/// What is chosen right now, stated once so no tab has to repeat it.
fn chosen(view: &View) -> Value {
    label(format!(
        "config: {}    profile: {}",
        if view.config.is_empty() { "— none picked —" } else { &view.config },
        if view.profile.is_empty() { "— the config decides —" } else { &view.profile },
    ))
}

// ─── the checkout ─────────────────────────────────────────────────────────────

pub fn repo(view: &View, survey: &Value) -> Value {
    let mut widgets = header(view);
    widgets.push(chosen(view));
    widgets.push(json!({
        "kind": "search-box", "id": "filter", "action": "filter",
        "value": view.filter, "placeholder": "filter configs, profiles, artifacts",
    }));

    widgets.push(section("The checkout", true));
    widgets.push(label(survey["root"].as_str().unwrap_or("?").to_string()));
    // ⛔ SAY THIS ON THE FIRST SCREEN, NOT FORTY MINUTES IN. `lb` missing means
    //    a real build cannot run on this machine at all, and the plan flow will
    //    still happily show the command lines it would have run.
    widgets.push(label(if survey["live_build_present"].as_bool().unwrap_or(false) {
        "live-build (`lb`) is installed — a real build can run here".to_string()
    } else {
        "⛔ live-build (`lb`) is NOT installed — the plan is still honest, but no \
         real build can run on this machine".to_string()
    }));
    widgets.push(label(format!(
        "{} hooks · {} package lists",
        survey["hooks_count"].as_u64().unwrap_or(0),
        survey["package_lists"].as_array().map(Vec::len).unwrap_or(0),
    )));

    // ⛔ A LIST THAT COULD NOT BE READ IS NOT AN EMPTY LIST. The profiles come
    //    from parsing the shell script's own usage block, on purpose, so there
    //    is no second list here to fall out of step — and when that parse fails
    //    the pane says so rather than showing nothing, which would read as
    //    "this build has no profiles".
    widgets.push(section("Profiles", false));
    match survey["profiles_error"].as_str() {
        Some(why) => widgets.push(label(format!(
            "⛔ the profile list could not be read from the build script: {why}. \
             Nothing below is a profile list."
        ))),
        None => {
            for p in survey["profiles"].as_array().cloned().unwrap_or_default() {
                let name = p["name"].as_str().unwrap_or("?");
                if !matches(&view.filter, &[name]) {
                    continue;
                }
                let composite = p["composite"].as_bool().unwrap_or(false);
                widgets.push(json!({
                    "kind": "list-row",
                    "id": format!("profile:{name}"),
                    "title": name,
                    "subtitle": if composite {
                        "expands to several real builds, so the plan shows more than one command"
                    } else { "one image" },
                    "selected": view.profile == name,
                    "row_action": format!("pick-profile:{name}"),
                }));
            }
        }
    }

    widgets.push(section("Configs", false));
    for c in survey["configs"].as_array().cloned().unwrap_or_default() {
        let name = c["name"].as_str().unwrap_or("?");
        let summary = c["summary"].as_str().unwrap_or("");
        if !matches(&view.filter, &[name, summary]) {
            continue;
        }
        widgets.push(json!({
            "kind": "list-row",
            "id": format!("config:{name}"),
            "title": if c["is_default"].as_bool().unwrap_or(false) {
                format!("{name}  (the default)")
            } else { name.to_string() },
            "subtitle": format!(
                "{} knobs{}{}",
                c["knob_count"].as_u64().unwrap_or(0),
                if summary.is_empty() { "" } else { " · " },
                summary,
            ),
            "selected": view.config == name,
            "row_action": format!("pick-config:{name}"),
        }));
    }

    let artifacts = survey["artifacts"].as_array().cloned().unwrap_or_default();
    widgets.push(section("Artifacts already built", false));
    if artifacts.is_empty() {
        widgets.push(label("none in this checkout yet"));
    }
    for a in artifacts {
        let name = a["name"].as_str().unwrap_or("?");
        if !matches(&view.filter, &[name]) {
            continue;
        }
        widgets.push(json!({
            "kind": "list-row",
            "id": format!("artifact:{name}"),
            "title": name,
            "subtitle": size(a["bytes"].as_u64().unwrap_or(0)),
            // An image on disk outlives the app that made it.
            "status": "durable",
        }));
    }

    json!({
        "title": "Yggdrasil Maker — the checkout",
        "widgets": widgets,
        "footer": [
            label("pick a config and a profile, then open Plan"),
            json!({"kind": "button", "id": "to-plan", "action": "tab-plan",
                   "label": "Plan this build", "primary": true}),
        ],
    })
}

// ─── the plan ─────────────────────────────────────────────────────────────────

/// ⭐ THE PLAN IS THE PRODUCT'S PROMISE, RENDERED. The repo's own contract says
/// *"the maker shows you the command lines it would run, so you can run them
/// yourself"* — so the command line is the row's subtitle, in full, not hidden
/// behind a disclosure. An app that hides the shell it is wrapping has become
/// the gate it promised not to be.
pub fn plan(view: &View, built: Option<&Value>, error: Option<&str>) -> Value {
    let mut widgets = header(view);
    widgets.push(chosen(view));
    widgets.push(json!({
        "kind": "toggle", "id": "skip-smoke", "action": "skip-smoke",
        "label": "skip the smoke test", "value": view.skip_smoke,
    }));

    if let Some(why) = error {
        widgets.push(section("This plan could not be built", true));
        widgets.push(label(format!("⛔ {why}")));
        return json!({"title": "Yggdrasil Maker — plan", "widgets": widgets,
                      "footer": [label("fix the selection on the Checkout tab")]});
    }
    let Some(p) = built else {
        widgets.push(label(
            "pick a config on the Checkout tab; the plan is built from it and from \
             the build script, never from anything remembered here.",
        ));
        return json!({"title": "Yggdrasil Maker — plan", "widgets": widgets, "footer": []});
    };

    let effective = p["effective_profiles"].as_array().cloned().unwrap_or_default();
    widgets.push(section(
        format!(
            "{} · {}",
            p["config"].as_str().unwrap_or("?"),
            effective.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" + "),
        ),
        true,
    ));
    // ⛔ NAME THE SURPRISE. The build script honours an environment profile when
    //    none is passed, and a user who does not know that cannot read the
    //    command lines below.
    if let Some(from_config) = p["profile_from_config"].as_str() {
        widgets.push(label(format!(
            "the profile came from the config, not from your pick: {from_config}"
        )));
    }

    widgets.push(section("Steps", false));
    for s in p["steps"].as_array().cloned().unwrap_or_default() {
        let command = s["command"]
            .as_array()
            .map(|c| c.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        widgets.push(json!({
            "kind": "list-row",
            "id": format!("step:{}", s["index"].as_u64().unwrap_or(0)),
            "title": format!("{}. {}", s["index"].as_u64().unwrap_or(0),
                             s["title"].as_str().unwrap_or("?")),
            "subtitle": if command.is_empty() {
                format!("{} · {}", s["cost"].as_str().unwrap_or("?"),
                        s["note"].as_str().unwrap_or(""))
            } else {
                format!("{command}    ({}) {}", s["cost"].as_str().unwrap_or("?"),
                        s["note"].as_str().unwrap_or(""))
            },
        }));
    }

    let deltas = p["deltas"].as_array().cloned().unwrap_or_default();
    widgets.push(section("What this config changes from the shipped example", false));
    if deltas.is_empty() {
        widgets.push(label("nothing — this is the example, unmodified"));
    }
    for d in deltas {
        widgets.push(json!({
            "kind": "list-row",
            "id": format!("delta:{}", d["key"].as_str().unwrap_or("?")),
            "title": d["key"].as_str().unwrap_or("?"),
            "subtitle": format!(
                "{}: {} → {}",
                d["kind"].as_str().unwrap_or("?"),
                d["baseline"].as_str().unwrap_or("(unset)"),
                d["value"].as_str().unwrap_or("(unset)"),
            ),
        }));
    }

    json!({
        "title": "Yggdrasil Maker — plan",
        "widgets": widgets,
        "footer": [
            label("everything above is a shell command you can run yourself"),
            json!({"kind": "button", "id": "start", "action": "start",
                   "label": "Run the first stage", "primary": true}),
        ],
    })
}

// ─── the build ────────────────────────────────────────────────────────────────

/// ⛔ THE LOG IS THE POINT, AND IT IS TAILED, NOT TRUNCATED SILENTLY. A build
/// log outgrows any pane; showing the FIRST lines would show the part nobody
/// needs. The tail is shown and the count is stated, so a reader knows what is
/// above rather than assuming there is nothing.
const LOG_TAIL: usize = 40;

pub fn build(view: &View, status: &Value) -> Value {
    let mut widgets = header(view);
    let state = status["state"].as_str().unwrap_or("idle");
    widgets.push(section(
        match state {
            "running" => "Running",
            "done" => "Finished",
            "failed" => "⛔ Failed",
            _ => "Nothing has been run",
        },
        true,
    ));

    let command = status["command"]
        .as_array()
        .map(|c| c.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    if !command.is_empty() {
        widgets.push(label(command));
    }
    if let Some(code) = status["exit_code"].as_i64() {
        widgets.push(label(format!("exit code {code}")));
    }
    // ⛔ "COULD NOT START" IS NOT "FAILED". A process that never spawned has no
    //    log to read and no exit code to explain it; say which happened.
    if let Some(why) = status["error"].as_str() {
        widgets.push(label(format!("⛔ it never started: {why}")));
    }

    let lines = status["lines"].as_array().cloned().unwrap_or_default();
    let shown: Vec<&Value> = lines.iter().rev().take(LOG_TAIL).rev().collect();
    widgets.push(section(
        if lines.len() > shown.len() {
            format!("Output — the last {} of {} lines", shown.len(), lines.len())
        } else {
            format!("Output — {} lines", lines.len())
        },
        false,
    ));
    if shown.is_empty() {
        widgets.push(label("nothing yet"));
    }
    let mut log = String::new();
    for line in shown {
        log.push_str(line["text"].as_str().unwrap_or(""));
        log.push('\n');
    }
    if !log.is_empty() {
        // Fenced, so the host renders it as preformatted output rather than
        // reflowing a build log into prose.
        widgets.push(json!({
            "kind": "markdown", "id": "log",
            "source": format!("```\n{log}```\n"),
        }));
    }

    json!({
        "title": "Yggdrasil Maker — build",
        "widgets": widgets,
        "footer": [
            label(if state == "running" {
                "a stage is running; starting another is refused rather than interleaved"
            } else {
                "runs the cheap real stage — the same command the plan shows"
            }),
            json!({"kind": "button", "id": "start", "action": "start",
                   "label": "Run the first stage", "primary": true}),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn survey() -> Value {
        json!({
            "root": "/home/user/example-checkout",
            "profiles": [{"name": "example-a", "composite": false},
                         {"name": "example-both", "composite": true}],
            "profiles_error": null,
            "configs": [{"name": "example.toml", "path": "/x", "summary": "an invented config",
                         "knob_count": 4, "is_default": true}],
            "live_build_present": false,
            "artifacts": [{"name": "example-image.iso", "bytes": 2_500_000_000u64}],
            "hooks_count": 3,
            "package_lists": ["one", "two"],
        })
    }

    /// ⛔ THE CONTRACT IS FIELD NAMES, AND A HAND-BUILT SCHEMA CANNOT BE CHECKED
    /// BY THE COMPILER. Same splint as the sibling fleet app carries, for the
    /// same reason, and it should be deleted the day the widget schema exists as
    /// a typed crate. SSOT: `AppPaneWidget` in yggterm's shell crate.
    #[test]
    fn every_widget_uses_the_names_the_host_deserialises() {
        let required: &[(&str, &[&str])] = &[
            ("section", &["text"]),
            ("label", &["text"]),
            ("tabs", &["id", "tabs", "active"]),
            ("search-box", &["id"]),
            ("toggle", &["id", "label"]),
            ("button", &["id", "label", "action"]),
            ("list-row", &["id", "title"]),
            ("markdown", &["id", "source"]),
        ];
        let view = View::default();
        let plan_value = json!({
            "config": "example.toml", "requested_profile": "example-a",
            "effective_profiles": ["example-a"], "profile_from_config": null,
            "steps": [{"index": 1, "title": "configure", "command": ["./example.sh", "--go"],
                       "note": "invented", "cost": "cheap"}],
            "env": [], "deltas": [],
        });
        let status = json!({"state": "running", "command": ["./example.sh"],
                            "lines": [{"text": "hello"}], "exit_code": null, "error": null});
        for schema in [
            repo(&view, &survey()),
            plan(&view, Some(&plan_value), None),
            build(&view, &status),
        ] {
            assert!(schema["title"].is_string());
            let footer = schema["footer"].as_array().cloned().unwrap_or_default();
            for widget in schema["widgets"].as_array().unwrap().iter().chain(footer.iter()) {
                let kind = widget["kind"].as_str().unwrap();
                let fields = required.iter().find(|(k, _)| *k == kind)
                    .unwrap_or_else(|| panic!("no contract recorded for kind {kind}")).1;
                for field in fields {
                    assert!(widget.get(field).is_some(),
                            "{kind} is missing `{field}` — the pane would refuse to render");
                }
                assert!(widget.get("menu").is_none(), "this app declines context menus");
                if kind == "section" {
                    assert!(widget.get("title").is_none(), "a section's heading is `text`");
                }
                if kind == "tabs" {
                    assert!(widget.get("selected").is_none(), "tabs select with `active`");
                }
            }
        }
    }

    #[test]
    fn a_missing_live_build_is_announced_on_the_first_screen() {
        // Learning this forty minutes into a build is the failure the line exists
        // to prevent.
        let text = repo(&View::default(), &survey()).to_string();
        assert!(text.contains("is NOT installed"));
    }

    #[test]
    fn an_unreadable_profile_list_is_not_rendered_as_no_profiles() {
        let mut s = survey();
        s["profiles_error"] = json!("the usage block no longer matches");
        s["profiles"] = json!([]);
        let text = repo(&View::default(), &s).to_string();
        assert!(text.contains("could not be read"));
        assert!(text.contains("Nothing below is a profile list"));
    }

    #[test]
    fn a_composite_profile_says_why_it_produces_more_than_one_command() {
        let text = repo(&View::default(), &survey()).to_string();
        assert!(text.contains("expands to several real builds"));
    }

    #[test]
    fn the_plan_shows_the_command_line_in_full() {
        // The repo's own contract: the maker shows you the command lines it
        // would run, so you can run them yourself.
        let plan_value = json!({
            "config": "example.toml", "requested_profile": "example-a",
            "effective_profiles": ["example-a"], "profile_from_config": null,
            "steps": [{"index": 1, "title": "configure",
                       "command": ["./example.sh", "--config", "example.toml"],
                       "note": "invented", "cost": "cheap"}],
            "env": [], "deltas": [],
        });
        let text = plan(&View::default(), Some(&plan_value), None).to_string();
        assert!(text.contains("./example.sh --config example.toml"));
    }

    #[test]
    fn a_process_that_never_started_is_not_reported_as_a_failed_build() {
        let status = json!({"state": "failed", "command": [], "lines": [],
                            "exit_code": null, "error": "no such file"});
        let text = build(&View::default(), &status).to_string();
        assert!(text.contains("it never started"));
    }

    #[test]
    fn a_truncated_log_says_what_is_above_it() {
        let lines: Vec<Value> = (0..200).map(|i| json!({"text": format!("line {i}")})).collect();
        let status = json!({"state": "done", "command": [], "lines": lines,
                            "exit_code": 0, "error": null});
        let text = build(&View::default(), &status).to_string();
        assert!(text.contains("the last 40 of 200 lines"));
    }
}
