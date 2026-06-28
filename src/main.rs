//! ventouse CLI: discover Python files under a path, analyze them as one project, render.
//!
//! Usage: `ventouse [PATH] [--format text|json] [--summary | --all] [--error]`
//! (PATH defaults to `.`; view defaults to summary.)

use std::process::ExitCode;

use ventouse::config::weights_from_pyproject;
use ventouse::core::{Category, DeclOrder, Severity};
use ventouse::discover::{cpp_files, source_files, ts_files};
use ventouse::lang::{cpp, python, rust, ts};
use ventouse::render::{By, Format, View, render};

#[derive(Clone, Copy, PartialEq)]
enum Lang {
    Python,
    Rust,
    Cpp,
    Ts,
}

struct Args {
    path: String,
    format: Format,
    view: View,
    error_exit: bool,
    lang: Option<Lang>,
    order: DeclOrder,
}

fn usage() -> String {
    "usage: ventouse [PATH] [--lang=python|rust|cpp|ts] [--order=bottom-up|top-down] [--format=text|json] [--summary|--all|--top=N --by=function|class|file] [--error]"
        .to_string()
}

fn parse_args() -> Result<Args, String> {
    // Parse straight into the result (mutate one `Args`) instead of a bag of `let mut` accumulators
    // — on ventouse's own `CrowdedScope` suggestion. `top_n`/`by` stay separate: they compute `view`.
    let mut a = Args {
        path: ".".to_string(),
        format: Format::Text,
        view: View::Summary,
        error_exit: false,
        lang: None,
        order: DeclOrder::BottomUp,
    };
    let mut top_n: Option<usize> = None;
    let mut by = By::Function;

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--summary" => a.view = View::Summary,
            "--all" => a.view = View::All,
            "--format=text" => a.format = Format::Text,
            "--format=json" => a.format = Format::Json,
            "--error" => a.error_exit = true,
            "--lang=python" => a.lang = Some(Lang::Python),
            "--lang=rust" => a.lang = Some(Lang::Rust),
            "--lang=cpp" => a.lang = Some(Lang::Cpp),
            "--lang=ts" | "--lang=typescript" | "--lang=js" => a.lang = Some(Lang::Ts),
            "--order=bottom-up" => a.order = DeclOrder::BottomUp,
            "--order=top-down" => a.order = DeclOrder::TopDown,
            "-h" | "--help" => return Err(usage()),
            s if s.starts_with("--top=") => {
                top_n = Some(
                    s["--top=".len()..]
                        .parse()
                        .map_err(|_| format!("--top= needs a number\n{}", usage()))?,
                );
            }
            s if s.starts_with("--by=") => {
                by = match &s["--by=".len()..] {
                    "function" => By::Function,
                    "class" => By::Class,
                    "file" => By::File,
                    other => return Err(format!("unknown --by={other}\n{}", usage())),
                };
            }
            s if s.starts_with("--") => return Err(format!("unknown flag: {s}\n{}", usage())),
            s => a.path = s.to_string(),
        }
    }
    if let Some(n) = top_n {
        a.view = View::Top { n, by };
    }
    Ok(a)
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::FAILURE;
        }
    };

    // Pick the language: explicit `--lang`, else whichever has the most files under the path.
    let lang = args.lang.unwrap_or_else(|| {
        let counts = [
            (Lang::Python, source_files(&args.path, "py").len()),
            (Lang::Rust, source_files(&args.path, "rs").len()),
            (Lang::Cpp, cpp_files(&args.path).len()),
            (Lang::Ts, ts_files(&args.path).len()),
        ];
        // most files wins; Python breaks ties (the original default).
        counts.iter().filter(|(_, n)| *n > 0).max_by_key(|(_, n)| *n).map(|(l, _)| *l).unwrap_or(Lang::Python)
    });

    let paths = match lang {
        Lang::Python => source_files(&args.path, "py"),
        Lang::Rust => source_files(&args.path, "rs"),
        Lang::Cpp => cpp_files(&args.path),
        Lang::Ts => ts_files(&args.path),
    };
    let mut sources: Vec<(String, String)> = Vec::new();
    for p in &paths {
        match std::fs::read_to_string(p) {
            Ok(src) => sources.push((p.clone(), src)),
            Err(e) => eprintln!("skip {p}: {e}"),
        }
    }
    let refs: Vec<(&str, &str)> = sources.iter().map(|(p, s)| (p.as_str(), s.as_str())).collect();

    let mut weights = weights_from_pyproject(&args.path);
    weights.order = args.order; // CLI flag wins over config
    let findings = match lang {
        Lang::Python => python::analyze_project(&refs, &weights),
        Lang::Rust => rust::analyze_project(&refs, &weights),
        Lang::Cpp => cpp::analyze_project(&refs, &weights),
        Lang::Ts => ts::analyze_project(&refs, &weights),
    };
    print!("{}", render(&findings, args.view, args.format));

    if args.error_exit
        && findings
            .iter()
            .any(|f| f.severity == Severity::Error && f.category == Category::ParseError)
    {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
