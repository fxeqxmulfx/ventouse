//! Render + discovery. Render is tested from fixed findings (decoupled from the core); plus an
//! end-to-end pass for the headline scope-debt total and ParseError wiring.

use ventouse::core::{Category, EntityKind, Finding, Reason, Severity, Weights};
use ventouse::discover::python_files;
use ventouse::lang::python::analyze_project;
use ventouse::render::{By, Format, View, rank, render, summarize};

fn scope(file: &str, line: u32, entity: &str, kind: EntityKind, score: u32) -> Finding {
    Finding {
        file: file.to_string(),
        line,
        entity: entity.to_string(),
        entity_kind: kind,
        category: Category::ScopeDebt,
        severity: Severity::Info,
        score,
        reason: Reason::ExcessLevels(score / 10),
    }
}

fn sample() -> Vec<Finding> {
    vec![
        scope("a.py", 2, "f.x", EntityKind::Binding, 20),
        scope("b.py", 5, "g.y", EntityKind::Binding, 20),
    ]
}

#[test]
fn summary_rollup() {
    let s = summarize(&sample());
    assert_eq!(s.scope_total, 40);
    assert_eq!(s.combined(), 40);
    assert_eq!(s.per_file.len(), 2);
}

#[test]
fn text_summary_mentions_total() {
    let out = render(&sample(), View::Summary, Format::Text);
    assert!(out.contains("scope-debt 40"), "{out}");
}

#[test]
fn text_all_lists_findings_sorted() {
    let out = render(&sample(), View::All, Format::Text);
    let a = out.find("a.py").unwrap();
    let b = out.find("b.py").unwrap();
    assert!(a < b, "sorted by file:\n{out}");
    assert!(out.contains("declared 2 level(s) too high"), "{out}");
}

#[test]
fn empty_all_is_clean() {
    assert!(render(&[], View::All, Format::Text).contains("clean"));
}

#[test]
fn reorder_binding_reason_renders_the_move_hint() {
    let f = Finding {
        file: "t.py".into(), line: 2, entity: "f.q".into(), entity_kind: EntityKind::Binding,
        category: Category::Suggestion, severity: Severity::Info, score: 0,
        reason: Reason::ReorderBinding { first_use: 30, wedged: 3 },
    };
    let out = render(&[f], View::All, Format::Text);
    assert!(out.contains("3 unrelated definition(s) before its first use (line 30)"), "{out}");
    assert!(out.contains("move the declaration down to it"), "{out}");
}

#[test]
fn forward_ref_reason_names_the_callee_not_the_referrer() {
    // declare-order findings are attributed to the REFERRER, so the wording must name the callee
    // (declared below) rather than read "<referrer> used before its declaration" (which is false —
    // the referrer is in place). The reason carries the callee name.
    let f = Finding {
        file: "t.py".into(), line: 1, entity: "f1".into(), entity_kind: EntityKind::Function,
        category: Category::DeclBeforeUse, severity: Severity::Warning, score: 0,
        reason: Reason::ForwardRef { callee: "f2".into(), below: true },
    };
    let out = render(&[f], View::All, Format::Text);
    assert!(out.contains("references `f2`, declared below it"), "{out}");
    assert!(!out.contains("used before its declaration"), "{out}");
}

#[test]
fn json_summary_has_keys() {
    let out = render(&sample(), View::Summary, Format::Json);
    assert!(out.contains("\"combined\": 40"), "{out}");
    assert!(out.contains("\"scope_total\": 40"));
    assert!(out.trim_start().starts_with('{'));
}

#[test]
fn json_all_is_array() {
    let out = render(&sample(), View::All, Format::Json);
    assert!(out.trim_start().starts_with('['));
    assert!(out.contains("\"entity\": \"f.x\""));
    assert!(out.contains("\"category\": \"ScopeDebt\""));
}

#[test]
fn discovery_finds_py_excludes_caches() {
    let files = python_files("tests/fixtures/python/examples");
    assert!(files.iter().any(|f| f.ends_with("good.py")));
    assert!(files.iter().all(|f| !f.contains("__pycache__")));
}

// --- ranking views (--top --by) ---------------------------------------------------------

fn ranked_sample() -> Vec<Finding> {
    vec![
        scope("a.py", 1, "big", EntityKind::Function, 100),
        scope("a.py", 2, "big.tmp", EntityKind::Binding, 5), // rolls into `big`
        scope("a.py", 3, "small", EntityKind::Function, 20),
        scope("b.py", 1, "Repo", EntityKind::Class, 10),
        scope("b.py", 2, "Repo.save", EntityKind::Method, 40),
    ]
}

#[test]
fn rank_by_function_rolls_in_bindings() {
    let r = rank(&ranked_sample(), By::Function);
    assert_eq!(r[0], ("big".to_string(), 105)); // 100 + 5 binding
    assert_eq!(r[1].0, "Repo.save");
    assert_eq!(r[1].1, 40);
}

#[test]
fn rank_by_class_rolls_in_methods() {
    let r = rank(&ranked_sample(), By::Class);
    assert_eq!(r[0], ("Repo".to_string(), 50)); // class 10 + save 40
}

#[test]
fn rank_by_file() {
    let r = rank(&ranked_sample(), By::File);
    assert_eq!(r[0], ("a.py".to_string(), 125)); // 100+5+20
    assert_eq!(r[1], ("b.py".to_string(), 50));
}

#[test]
fn top_view_truncates_and_renders() {
    let out = render(&ranked_sample(), View::Top { n: 1, by: By::Function }, Format::Text);
    assert!(out.contains("top 1 by function"), "{out}");
    assert!(out.contains("big"), "{out}");
    assert!(!out.contains("small"), "should be truncated to 1:\n{out}");
}

#[test]
fn top_view_json() {
    let out = render(&ranked_sample(), View::Top { n: 2, by: By::File }, Format::Json);
    assert!(out.trim_start().starts_with('['));
    assert!(out.contains("\"name\": \"a.py\""));
    assert!(out.contains("\"score\": 125"));
}

// --- end-to-end pipeline -----------------------------------------------------------------

#[test]
fn good_example_has_no_scope_debt() {
    let src = std::fs::read_to_string("tests/fixtures/python/examples/good.py").unwrap();
    let findings = analyze_project(&[("good.py", &src)], &Weights::default());
    let s = summarize(&findings);
    assert_eq!(s.scope_total, 0, "good.py is the ideal — no scope-debt:\n{findings:#?}");
    assert_eq!(s.parse_errors, 0);
}

#[test]
fn parse_error_is_reported_and_others_still_analyzed() {
    let bad = "def f(:\n";
    let ok = "def g():\n    return 1\n";
    let findings = analyze_project(&[("bad.py", bad), ("ok.py", ok)], &Weights::default());
    assert_eq!(findings.iter().filter(|f| f.category == Category::ParseError).count(), 1);
    assert!(findings.iter().any(|f| f.file == "bad.py" && f.category == Category::ParseError));
}
