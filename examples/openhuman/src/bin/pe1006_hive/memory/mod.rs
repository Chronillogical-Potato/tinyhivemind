//! A markdown file as the hive's working memory, with compaction.
//!
//! This is the reference [`WorkingMemory`] adapter for the examples: the
//! simplest engine that can carry a hive's observations across activations
//! and runs. It is deliberately not the library's opinion. A host with a
//! vector store or a memory service implements the same port and the seats
//! never notice.
//!
//! The file is human-readable and human-editable:
//!
//! ```text
//! # Hive memory
//! <!-- hive-memory:v1 next=14 -->
//! ## Compacted
//! - m3 | hive | solver | the brute force over k<=12 agrees with the sample…
//! ## Recent
//! - m13 | hive | checker | held-out k=14 mismatch: recurrence of order 60 fails
//! ```
//!
//! **Compaction.** `Recent` keeps entries verbatim. When it passes
//! [`Compaction::max_recent`], the oldest fold into `Compacted` as one-line
//! excerpts, exact repeats collapse into the newest copy, and the oldest
//! compacted excerpts fall off past [`Compaction::max_compacted`]. No model is
//! involved, so compaction is deterministic and free.
//!
//! **Concurrency.** Each seat's tool server is its own process, so every
//! operation holds a lock file for its read-modify-write and saves by atomic
//! rename. Seats may write at once without losing each other's notes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tinyhivemind_core::runtime::{
    BoxError, MemoryEntry, MemoryFuture, MemoryNote, MemoryQuery, MemoryScope, WorkingMemory,
};

const HEADER: &str = "# Hive memory\n";
const MARKER: &str = "<!-- hive-memory:v1 next=";
const LOCK_ATTEMPTS: u32 = 200;
const LOCK_PAUSE: Duration = Duration::from_millis(15);
const LOCK_STALE: Duration = Duration::from_secs(10);

/// When and how the file folds old entries away.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Compaction {
    /// Entries kept verbatim after a fold.
    pub keep_recent: usize,
    /// Verbatim entries that trigger a fold.
    pub max_recent: usize,
    /// Characters an entry keeps once compacted.
    pub excerpt_chars: usize,
    /// Compacted excerpts retained; the oldest fall off.
    pub max_compacted: usize,
}

impl Compaction {
    pub(super) const DEFAULT: Self = Self {
        keep_recent: 12,
        max_recent: 24,
        excerpt_chars: 140,
        max_compacted: 60,
    };
}

/// The hive's memory as one markdown file.
#[derive(Clone, Debug)]
pub(super) struct MarkdownMemory {
    path: PathBuf,
    compaction: Compaction,
}

#[derive(Default)]
struct Store {
    next: u64,
    compacted: Vec<MemoryEntry>,
    recent: Vec<MemoryEntry>,
}

impl MarkdownMemory {
    pub(super) fn new(path: impl Into<PathBuf>, compaction: Compaction) -> Self {
        Self {
            path: path.into(),
            compaction,
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    fn with_store<T>(&self, work: impl FnOnce(&mut Store) -> T) -> io::Result<T> {
        let _lock = Lock::take(&self.path.with_extension("md.lock"))?;
        let mut store = match fs::read_to_string(&self.path) {
            Ok(text) => Store::parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Store::default(),
            Err(error) => return Err(error),
        };
        let out = work(&mut store);
        let staged = self.path.with_extension("md.tmp");
        fs::write(&staged, store.render())?;
        fs::rename(&staged, &self.path)?;
        Ok(out)
    }

    fn record_now(&self, note: &MemoryNote) -> io::Result<MemoryEntry> {
        let policy = self.compaction;
        self.with_store(|store| store.record(note, policy))
    }

    fn recall_now(&self, query: &MemoryQuery) -> io::Result<Vec<MemoryEntry>> {
        self.with_store(|store| store.recall(query))
    }

    fn forget_now(&self, seat: &str, id: &str) -> io::Result<()> {
        self.with_store(|store| store.forget(seat, id))
    }
}

impl WorkingMemory for MarkdownMemory {
    fn recall<'a>(&'a self, query: &'a MemoryQuery) -> MemoryFuture<'a, Vec<MemoryEntry>> {
        Box::pin(async move { self.recall_now(query).map_err(|e| Box::new(e) as BoxError) })
    }

    fn record<'a>(&'a self, note: &'a MemoryNote) -> MemoryFuture<'a, MemoryEntry> {
        Box::pin(async move { self.record_now(note).map_err(|e| Box::new(e) as BoxError) })
    }

    fn forget<'a>(&'a self, seat: &'a str, id: &'a str) -> MemoryFuture<'a, ()> {
        Box::pin(async move { self.forget_now(seat, id).map_err(|e| Box::new(e) as BoxError) })
    }
}

impl Store {
    fn record(&mut self, note: &MemoryNote, policy: Compaction) -> MemoryEntry {
        let text = one_line(&note.text);
        let same = |entry: &&MemoryEntry| {
            entry.scope == note.scope
                && entry.author == note.author
                && fold(&entry.text) == fold(&text)
        };
        if let Some(existing) = self.recent.iter().find(same) {
            return existing.clone();
        }
        let entry = MemoryEntry {
            id: format!("m{}", self.next.max(1)),
            author: note.author.clone(),
            scope: note.scope,
            text,
        };
        self.next = self.next.max(1) + 1;
        self.recent.push(entry.clone());
        self.compact(policy);
        entry
    }

    fn compact(&mut self, policy: Compaction) {
        if self.recent.len() <= policy.max_recent {
            return;
        }
        let fold_count = self.recent.len() - policy.keep_recent.min(self.recent.len());
        for mut old in self.recent.drain(..fold_count).collect::<Vec<_>>() {
            old.text = excerpt(&old.text, policy.excerpt_chars);
            self.compacted.push(old);
        }
        // An exact repeat keeps only its newest copy.
        let mut kept: Vec<MemoryEntry> = Vec::new();
        for entry in self.compacted.drain(..).rev() {
            let repeated = kept.iter().any(|k| {
                k.scope == entry.scope && k.author == entry.author && fold(&k.text) == fold(&entry.text)
            });
            if !repeated {
                kept.push(entry);
            }
        }
        kept.reverse();
        let excess = kept.len().saturating_sub(policy.max_compacted);
        kept.drain(..excess);
        self.compacted = kept;
    }

    fn visible(entry: &MemoryEntry, seat: &str) -> bool {
        entry.scope == MemoryScope::Hive || entry.author == seat
    }

    fn recall(&self, query: &MemoryQuery) -> Vec<MemoryEntry> {
        let words = tokens(&query.query);
        let mut ranked: Vec<(usize, u64, &MemoryEntry)> = self
            .compacted
            .iter()
            .chain(&self.recent)
            .filter(|entry| Self::visible(entry, &query.seat))
            .map(|entry| {
                let text = entry.text.to_lowercase();
                let hits = words.iter().filter(|word| text.contains(word.as_str())).count();
                (hits, number(&entry.id), entry)
            })
            .collect();
        if !words.is_empty() && ranked.iter().any(|(hits, ..)| *hits > 0) {
            ranked.retain(|(hits, ..)| *hits > 0);
        }
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        ranked
            .into_iter()
            .take(query.limit)
            .map(|(_, _, entry)| entry.clone())
            .collect()
    }

    fn forget(&mut self, seat: &str, id: &str) {
        let keep = |entry: &MemoryEntry| entry.id != id || !Self::visible(entry, seat);
        self.compacted.retain(keep);
        self.recent.retain(keep);
    }

    fn parse(text: &str) -> Self {
        let mut store = Self::default();
        let mut into_compacted = false;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix(MARKER) {
                store.next = rest.trim_end_matches("-->").trim().parse().unwrap_or(0);
            } else if line.starts_with("## Compacted") {
                into_compacted = true;
            } else if line.starts_with("## Recent") {
                into_compacted = false;
            } else if let Some(entry) = line.strip_prefix("- ").and_then(parse_entry) {
                store.next = store.next.max(number(&entry.id) + 1);
                if into_compacted {
                    store.compacted.push(entry);
                } else {
                    store.recent.push(entry);
                }
            }
        }
        store
    }

    fn render(&self) -> String {
        let mut out = format!("{HEADER}{MARKER}{} -->\n\n## Compacted\n", self.next.max(1));
        for entry in &self.compacted {
            out.push_str(&line(entry));
        }
        out.push_str("\n## Recent\n");
        for entry in &self.recent {
            out.push_str(&line(entry));
        }
        out
    }
}

fn line(entry: &MemoryEntry) -> String {
    let scope = match entry.scope {
        MemoryScope::Hive => "hive",
        MemoryScope::Seat => "seat",
    };
    format!("- {} | {scope} | {} | {}\n", entry.id, entry.author, entry.text)
}

fn parse_entry(rest: &str) -> Option<MemoryEntry> {
    let mut parts = rest.splitn(4, " | ");
    let id = parts.next()?.trim();
    let scope = match parts.next()?.trim() {
        "hive" => MemoryScope::Hive,
        "seat" => MemoryScope::Seat,
        _ => return None,
    };
    let author = parts.next()?.trim();
    let text = parts.next()?.trim();
    (id.starts_with('m') && !text.is_empty()).then(|| MemoryEntry {
        id: id.to_owned(),
        author: author.to_owned(),
        scope,
        text: text.to_owned(),
    })
}

fn number(id: &str) -> u64 {
    id.trim_start_matches('m').parse().unwrap_or(0)
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn fold(text: &str) -> String {
    one_line(text).to_lowercase()
}

fn excerpt(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        return text.to_owned();
    }
    let cut: String = text.chars().take(chars).collect();
    format!("{}…", cut.trim_end())
}

fn tokens(query: &str) -> Vec<String> {
    query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.chars().count() >= 3)
        .map(str::to_lowercase)
        .collect()
}

/// A lock file held for one read-modify-write.
struct Lock(PathBuf);

impl Lock {
    fn take(path: &Path) -> io::Result<Self> {
        for _ in 0..LOCK_ATTEMPTS {
            match fs::OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(_) => return Ok(Self(path.to_owned())),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let stale = fs::metadata(path)
                        .and_then(|meta| meta.modified())
                        .ok()
                        .and_then(|at| SystemTime::now().duration_since(at).ok())
                        .is_some_and(|age| age > LOCK_STALE);
                    if stale {
                        let _ = fs::remove_file(path);
                    } else {
                        std::thread::sleep(LOCK_PAUSE);
                    }
                }
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(io::ErrorKind::TimedOut, "hive memory is locked"))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod test;
