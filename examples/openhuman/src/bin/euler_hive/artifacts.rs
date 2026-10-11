//! The run directory: every event, frame, transcript, program and search,
//! written as it happens so a run in progress can be followed.
//!
//! The directory is `<runs>/<problem>/<timestamp>/` outside the repository
//! (`hive-runs/` at the main checkout's root, git-ignored). Nothing written
//! here is ever committed: it holds the problem's data, the seats' working
//! and the hive's answer. Credentials are never written.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
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
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.root
    }

    /// Append one event line and flush it.
    pub(super) fn event(&self, value: &serde_json::Value) {
        append(&self.events, value);
    }

    /// Append one viewer frame and flush it, so the viewer can follow live.
    pub(super) fn frame(&self, frame: &impl Serialize) {
        if let Ok(value) = serde_json::to_value(frame) {
            append(&self.frames, &value);
        }
    }

    /// Write a whole file under the run directory.
    pub(super) fn write(&self, name: &str, contents: impl AsRef<[u8]>) {
        let path = self.root.join(name);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = std::fs::write(&path, contents) {
            eprintln!("[artifacts] could not write {}: {error}", path.display());
        }
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

    /// The next search number, for `search/NNN-<seat>.json`.
    pub(super) fn next_search(&self) -> u64 {
        self.searches.fetch_add(1, Ordering::Relaxed) + 1
    }
}

fn append(file: &Mutex<File>, value: &serde_json::Value) {
    if let Ok(mut file) = file.lock() {
        let _ = writeln!(file, "{value}");
        let _ = file.flush();
    }
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
