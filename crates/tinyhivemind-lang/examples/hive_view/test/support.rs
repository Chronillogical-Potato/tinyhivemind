//! Fixtures shared by the test modules.

use std::path::{Path, PathBuf};

use crate::Result;
use crate::episode::{Ending, Event};
use crate::load::load;
use crate::run::{Knobs, execute, prepare};

/// A gallery package's directory.
pub(super) fn gallery(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/hives")
        .join(name)
}

/// Default knobs at one seed.
pub(super) fn knobs(seed: u64) -> Knobs {
    Knobs {
        seed,
        ..Knobs::default()
    }
}

/// How a gallery package ends at one seed.
pub(super) fn ending(name: &str, seed: u64) -> Result<Ending> {
    let loaded = load(&gallery(name))?;
    let mut prepared = prepare(&loaded, &knobs(seed))?;
    Ok(execute(&mut prepared, None, &mut |_: Event<'_>| {})?.ending)
}
