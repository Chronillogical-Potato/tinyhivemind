//! Read a package directory into the document map `parse` takes.
//!
//! This is the one place the example touches the filesystem. The library stays
//! pure: it is handed strings and never opens a file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tinyhivemind_lang::{LoweredHive, Package, lower, parse};

use crate::Result;

/// A package read from disk, parsed, lowered, and its example-only sidecars.
#[derive(Debug)]
pub(crate) struct Loaded {
    /// The parsed package.
    pub(crate) package: Package,
    /// The lowered activation snapshot.
    pub(crate) lowered: LoweredHive,
    /// First paragraph of the package `README.md`, or empty.
    pub(crate) summary: String,
    /// `sim.json`, when the package carries one.
    pub(crate) sim: Option<Value>,
    /// `script.json`, when the package carries one.
    pub(crate) script: Option<BTreeMap<String, Vec<String>>>,
}

/// Read every file under `dir` into a map keyed by `/`-separated relative path.
///
/// # Errors
///
/// Fails when `dir` is not a directory, a file is not UTF-8, or `hive.json` is
/// missing.
pub(crate) fn documents(dir: &Path) -> Result<BTreeMap<String, String>> {
    if !dir.is_dir() {
        return Err(format!("{} is not a package directory", dir.display()).into());
    }
    let mut documents = BTreeMap::new();
    walk(dir, dir, &mut documents)?;
    if !documents.contains_key("hive.json") {
        return Err(format!("{} has no hive.json", dir.display()).into());
    }
    Ok(documents)
}

fn walk(root: &Path, dir: &Path, into: &mut BTreeMap<String, String>) -> Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<_>>()?;
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(root, &path, into)?;
            continue;
        }
        let relative = path.strip_prefix(root)?;
        let key: Vec<String> = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        into.insert(key.join("/"), std::fs::read_to_string(&path)?);
    }
    Ok(())
}

/// Read, parse and lower the package in `dir`.
///
/// # Errors
///
/// Fails on any read, parse, validation or sidecar decode error.
pub(crate) fn load(dir: &Path) -> Result<Loaded> {
    let documents = documents(dir)?;
    let package = parse(&documents)?;
    let lowered = lower(&package)?;
    let summary = documents
        .get("README.md")
        .map(|readme| summary(readme))
        .unwrap_or_default();
    let sim = documents
        .get("sim.json")
        .map(|source| serde_json::from_str(source))
        .transpose()?;
    let script = documents
        .get("script.json")
        .map(|source| serde_json::from_str(source))
        .transpose()?;
    Ok(Loaded {
        package,
        lowered,
        summary,
        sim,
        script,
    })
}

/// The first prose paragraph of a README, joined onto one line.
pub(crate) fn summary(readme: &str) -> String {
    let mut lines = Vec::new();
    for line in readme.lines() {
        let line = line.trim();
        if line.is_empty() {
            if lines.is_empty() {
                continue;
            }
            break;
        }
        if line.starts_with('#') && lines.is_empty() {
            continue;
        }
        lines.push(line);
    }
    lines.join(" ")
}

/// The body of a Markdown document after optional JSON frontmatter.
pub(crate) fn body(source: &str) -> &str {
    source
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map_or(source, |(_, body)| body)
}

/// The role each seat fills, keyed by seat id.
pub(crate) fn roles(lowered: &LoweredHive) -> BTreeMap<&str, &str> {
    lowered
        .candidates
        .iter()
        .map(|candidate| {
            (
                candidate.id.as_str(),
                candidate.role.as_deref().unwrap_or(""),
            )
        })
        .collect()
}

/// The desks each seat sits on, keyed by seat id, in declared desk order.
pub(crate) fn seat_desks(lowered: &LoweredHive) -> BTreeMap<&str, Vec<&str>> {
    let mut map: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for desk in &lowered.desks {
        for member in &desk.members {
            map.entry(member.as_str()).or_default().push(desk.id.as_str());
        }
    }
    map
}
