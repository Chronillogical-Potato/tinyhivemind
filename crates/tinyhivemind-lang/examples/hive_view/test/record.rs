//! Recording a run: frames go to the recording, flushed one line at a time,
//! whether or not stdout carries them, and `--record` parses only with a run.

use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;

use crate::Result;
use crate::frames::Frame;
use crate::output::FrameWriter;

/// A writer that keeps what was flushed apart from what was only written.
#[derive(Clone, Default)]
struct Probe {
    written: Rc<RefCell<Vec<u8>>>,
    flushed: Rc<RefCell<usize>>,
}

impl Write for Probe {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.written.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        *self.flushed.borrow_mut() = self.written.borrow().len();
        Ok(())
    }
}

fn end() -> Frame {
    Frame::End {
        ending: "converged".into(),
        topic: Some("merge".into()),
        correct: None,
        rounds: 1,
        turns: 2,
        desk: None,
    }
}

#[test]
fn a_recording_gets_every_frame_flushed_as_it_is_written() -> Result<()> {
    let probe = Probe::default();
    let mut writer = FrameWriter {
        stdout: true,
        kept: Some(Box::new(probe.clone())),
    };
    let mut out = Vec::new();
    writer.emit(&mut out, &end())?;
    let kept = String::from_utf8(probe.written.borrow().clone())?;
    assert_eq!(kept, String::from_utf8(out)?);
    assert_eq!(
        *probe.flushed.borrow(),
        kept.len(),
        "a follower must see the line now"
    );
    assert!(kept.starts_with(r#"{"type":"end""#) && kept.ends_with('\n'));
    Ok(())
}

#[test]
fn a_human_run_still_records_frames_but_prints_none() -> Result<()> {
    let probe = Probe::default();
    let mut writer = FrameWriter {
        stdout: false,
        kept: Some(Box::new(probe.clone())),
    };
    let mut out = Vec::new();
    writer.emit(&mut out, &end())?;
    assert_eq!((out).len(), 0);
    let kept = String::from_utf8(probe.written.borrow().clone())?;
    assert_eq!(kept.lines().count(), 1);
    Ok(())
}

#[test]
fn record_parses_as_a_flag() -> Result<()> {
    let parse = |argv: &[&str]| crate::parse_args(argv.iter().map(|arg| (*arg).to_owned()));
    assert!(parse(&["dir", "--run", "--record"])?.output.record);
    assert!(!parse(&["dir", "--run"])?.output.record);
    Ok(())
}

#[test]
fn a_run_label_is_one_safe_path_component() {
    use crate::output::run_label;
    for id in ["../../etc", "a/b", "/abs/path", "..", "x\\y"] {
        let label = run_label(id, 1);
        assert!(!label.contains('/') && !label.contains('\\'), "{label}");
        assert_eq!(
            std::path::Path::new(&label).components().count(),
            1,
            "{label}"
        );
        assert!(!label.starts_with('.'), "{label}");
    }
}

#[test]
fn run_labels_started_together_never_collide() {
    use crate::output::run_label;
    let labels: std::collections::BTreeSet<String> =
        (0..1000).map(|_| run_label("pkg", 1)).collect();
    assert_eq!(labels.len(), 1000);
    let prefix = |id: &str| run_label(id, 1).split("-seed").next().map(str::to_owned);
    assert_ne!(prefix("a/b"), prefix("a_b"));
}

#[test]
fn a_raised_halt_ends_the_run_before_the_next_round() -> Result<()> {
    use crate::episode::{Ending, Event};
    use crate::halt::{Halt, install};
    use crate::load::load;
    use crate::run::{execute, prepare};

    use super::support::{gallery, knobs};

    let loaded = load(&gallery("math-panel"))?;
    let rounds = |halting: bool| -> Result<(u32, bool, Ending)> {
        let mut prepared = prepare(&loaded, &knobs(7))?;
        let halt = Halt::default();
        install(&mut prepared, halt.clone());
        let outcome = execute(&mut prepared, None, &mut |event| {
            if halting && matches!(event, Event::Round(_)) {
                halt.stop();
            }
        })?;
        Ok((outcome.rounds, outcome.spent, outcome.ending))
    };
    let (full, _, _) = rounds(false)?;
    let (stopped, spent, ending) = rounds(true)?;
    assert!(full > 1, "the fixture must run more than one round");
    assert_eq!(stopped, 1);
    assert!(spent);
    assert_eq!(ending, Ending::Exhausted);
    Ok(())
}
