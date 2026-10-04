//! The `--trace out.jsonl` flag every example shares.

use std::fs::File;
use std::io::{BufWriter, Write};

use tinyhivemind_core::telemetry::Tracer;

use crate::{JsonlSink, TickClock};

/// The sink and clock a tracer borrows, built from the command line.
///
/// Without `--trace` the events are written to nowhere: tracing stays optional
/// and the example's own output is unchanged.
pub struct TraceRig {
    sink: JsonlSink<Box<dyn Write + Send>>,
    clock: TickClock,
    path: Option<String>,
}

impl TraceRig {
    /// Read `--trace <path>` from the process arguments.
    ///
    /// # Panics
    ///
    /// Panics when the trace file cannot be created; an example has no better
    /// recovery than saying so.
    #[must_use]
    pub fn from_args() -> Self {
        let mut args = std::env::args().skip(1);
        let mut path = None;
        while let Some(arg) = args.next() {
            if arg == "--trace" {
                path = args.next();
            }
        }
        let out: Box<dyn Write + Send> = match &path {
            Some(path) => Box::new(BufWriter::new(
                File::create(path).expect("the --trace path must be writable"),
            )),
            None => Box::new(std::io::sink()),
        };
        Self {
            sink: JsonlSink::new(out),
            clock: TickClock::default(),
            path,
        }
    }

    /// A tracer for one run, stamping with the deterministic clock.
    #[must_use]
    pub fn tracer(&self, run: &str) -> Tracer<'_> {
        Tracer::new(run, &self.sink, &self.clock)
    }

    /// Where the trace is going, when asked for.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }
}
