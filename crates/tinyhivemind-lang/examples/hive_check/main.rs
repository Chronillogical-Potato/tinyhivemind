//! Check a hive package, or an edit of one, the way a compiler checks source.
//!
//! ```sh
//! cargo run -p tinyhivemind-lang --example hive_check -- <original-dir> [<edited-dir>] [--json]
//! ```
//!
//! With one directory it diagnoses that package. With two it checks the
//! second as an edit of the first. Every finding prints as one line,
//! `file:pointer: rule: message`, or with `--json` as one JSON array; the
//! exit status is 1 when there is any finding and 2 for a usage error. This
//! is what an orchestrator runs after editing a package's files.
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
};
use tinyhivemind_lang::{Diagnostic, check, diagnose_documents};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const USAGE: &str = "usage: hive_check <original-dir> [<edited-dir>] [--json]";

/// Parsed command line: one or two package directories and the output form.
#[derive(Debug, PartialEq)]
struct Args {
    dirs: Vec<PathBuf>,
    json: bool,
}

fn args(raw: &[String]) -> Result<Args> {
    let mut parsed = Args {
        dirs: vec![],
        json: false,
    };
    for arg in raw {
        match arg.as_str() {
            "--json" => parsed.json = true,
            flag if flag.starts_with('-') => return Err(format!("unknown flag {flag}").into()),
            dir => parsed.dirs.push(dir.into()),
        }
    }
    if parsed.dirs.is_empty() || parsed.dirs.len() > 2 {
        return Err("expected one or two package directories".into());
    }
    Ok(parsed)
}

/// Read every file below `dir` into a map keyed by `/`-separated relative
/// path, skipping dot-files and dot-directories.
fn load(dir: &Path) -> Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![(dir.to_path_buf(), String::new())];
    while let Some((path, prefix)) = pending.pop() {
        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let key = format!("{prefix}{name}");
            if entry.file_type()?.is_dir() {
                pending.push((entry.path(), format!("{key}/")));
            } else {
                files.insert(key, std::fs::read_to_string(entry.path())?);
            }
        }
    }
    Ok(files)
}

/// Every finding for one package, or for an edit of `original`, and the
/// classes an accepted edit changed.
fn findings(
    original: &BTreeMap<String, String>,
    edited: Option<&BTreeMap<String, String>>,
) -> (Vec<Diagnostic>, Vec<String>) {
    let Some(edited) = edited else {
        return (diagnose_documents(original).1, vec![]);
    };
    match check(original, edited) {
        Ok(review) => {
            let classes = review
                .changes
                .iter()
                .map(|change| {
                    serde_json::to_value(change.class)
                        .ok()
                        .and_then(|class| class.as_str().map(str::to_owned))
                        .unwrap_or_default()
                })
                .collect();
            (vec![], classes)
        }
        Err(diagnostics) => (diagnostics, vec![]),
    }
}

/// Render findings as compiler lines or a JSON array.
fn render(diagnostics: &[Diagnostic], classes: &[String], json: bool) -> Result<String> {
    if json {
        return Ok(serde_json::to_string_pretty(diagnostics)? + "\n");
    }
    let mut text = String::new();
    for diagnostic in diagnostics {
        text.push_str(&diagnostic.to_string());
        text.push('\n');
    }
    let summary = match (diagnostics.len(), classes.is_empty()) {
        (0, true) => "ok: no findings".to_owned(),
        (0, false) => format!("ok: changes {}", classes.join(", ")),
        (1, _) => "1 finding".to_owned(),
        (count, _) => format!("{count} findings"),
    };
    text.push_str(&summary);
    text.push('\n');
    Ok(text)
}

/// Run the check for `raw` arguments, writing the report to `out`; report
/// whether the package or edit is clean.
fn run(raw: &[String], out: &mut impl Write) -> Result<bool> {
    let args = args(raw)?;
    let original = load(&args.dirs[0])?;
    let edited = args.dirs.get(1).map(|dir| load(dir)).transpose()?;
    let (diagnostics, classes) = findings(&original, edited.as_ref());
    out.write_all(render(&diagnostics, &classes, args.json)?.as_bytes())?;
    Ok(diagnostics.is_empty())
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    match run(&raw, &mut std::io::stdout()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            eprintln!("hive_check: {error}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod test;
