//! The run directory: every event, frame, transcript, program and search,
//! written as it happens so a run in progress can be followed.
//!
//! The directory is `<runs>/<problem>/<timestamp>/` outside the repository
//! (`hive-runs/` at the main checkout's root, git-ignored). Nothing written
//! here is ever committed: it holds the problem's data, the seats' working
//! and the hive's answer. Credentials are never written.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

/// The append-only files of one run.
#[derive(Debug)]
pub(super) struct RunDir {
    root: PathBuf,
    events: Mutex<File>,
    frames: Mutex<File>,
    programs: AtomicU64,
    searches: AtomicU64,
    turns: AtomicU64,
    /// The first write that failed: the run stops at the next round rather
    /// than spend on with a truncated record.
    failure: Mutex<Option<String>>,
}

impl RunDir {
    /// Create `root` and open its streams.
    ///
    /// # Errors
    ///
    /// Fails when the directory or a stream cannot be created.
    pub(super) fn create(root: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(root.join("sandbox"))?;
        std::fs::create_dir_all(root.join("search"))?;
        std::fs::create_dir_all(root.join("turns"))?;
        let open = |name: &str| {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(root.join(name))
        };
        Ok(Self {
            events: Mutex::new(open("events.ndjson")?),
            frames: Mutex::new(open("frames.ndjson")?),
            root,
            programs: AtomicU64::new(0),
            searches: AtomicU64::new(0),
            turns: AtomicU64::new(0),
            failure: Mutex::new(None),
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.root
    }

    /// Append one event line and flush it.
    pub(super) fn event(&self, value: &serde_json::Value) {
        let result = self
            .events
            .lock()
            .map_err(|_| std::io::Error::other("events stream poisoned"))
            .and_then(|mut file| append_to(&mut *file, value));
        self.note("events.ndjson", result);
    }

    /// Append one viewer frame and flush it, so the viewer can follow live.
    pub(super) fn frame(&self, frame: &impl Serialize) {
        let result = serde_json::to_value(frame)
            .map_err(std::io::Error::other)
            .and_then(|value| {
                self.frames
                    .lock()
                    .map_err(|_| std::io::Error::other("frames stream poisoned"))
                    .and_then(|mut file| append_to(&mut *file, &value))
            });
        self.note("frames.ndjson", result);
    }

    /// Remember the first failed write.
    pub(super) fn note(&self, what: &str, result: std::io::Result<()>) {
        if let Err(error) = result
            && let Ok(mut failure) = self.failure.lock()
            && failure.is_none()
        {
            *failure = Some(format!("{what}: {error}"));
        }
    }

    /// Whether every write so far landed.
    ///
    /// # Errors
    ///
    /// Returns the first write that failed.
    pub(super) fn check(&self) -> anyhow::Result<()> {
        match self.failure.lock().ok().and_then(|failure| failure.clone()) {
            Some(failure) => anyhow::bail!("the run record could not be written ({failure})"),
            None => Ok(()),
        }
    }

    /// Write a whole file under the run directory.
    pub(super) fn write(&self, name: &str, contents: impl AsRef<[u8]>) {
        let result = contained(name).and_then(|relative| {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, contents)
        });
        self.note(name, result);
    }

    /// Write a JSON file under the run directory.
    pub(super) fn json(&self, name: &str, value: &impl Serialize) {
        match serde_json::to_vec_pretty(value) {
            Ok(bytes) => self.write(name, bytes),
            Err(error) => eprintln!("[artifacts] could not encode {name}: {error}"),
        }
    }

    /// The next program number, for `sandbox/NNN-<seat>.*`.
    pub(super) fn next_program(&self) -> u64 {
        self.programs.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// The next turn number, for `turns/NNNN-<seat>.md`.
    pub(super) fn next_turn(&self) -> u64 {
        self.turns.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// The next search number, for `search/NNN-<seat>.json`.
    pub(super) fn next_search(&self) -> u64 {
        self.searches.fetch_add(1, Ordering::Relaxed) + 1
    }
}

/// Write one NDJSON line and flush it, reporting either failure.
pub(super) fn append_to(file: &mut impl Write, value: &serde_json::Value) -> std::io::Result<()> {
    writeln!(file, "{value}")?;
    file.flush()
}

/// `name` as a path that stays inside the run directory: relative, with no
/// parent, root or prefix components.
pub(super) fn contained(name: &str) -> std::io::Result<&Path> {
    let path = Path::new(name);
    if path
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        Ok(path)
    } else {
        Err(std::io::Error::other(format!("{name:?} would leave the run directory")))
    }
}

/// An id as one safe file-name component: anything but `[A-Za-z0-9_-]`
/// becomes `_`, so a seat id can never add a path separator or `..`.
pub(super) fn file_component(id: &str) -> String {
    let safe: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if safe.is_empty() { "_".into() } else { safe }
}

/// A short suffix unique across processes started in the same second: the
/// process id and the sub-second clock.
pub(super) fn unique_suffix() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());
    format!("{:x}{nanos:08x}", std::process::id())
}

/// A timestamped run directory name: `YYYYMMDDTHHMMSSZ`.
pub(super) fn timestamp() -> String {
    tinymemory_api::chrono::Utc::now()
        .format("%Y%m%dT%H%M%SZ")
        .to_string()
}
