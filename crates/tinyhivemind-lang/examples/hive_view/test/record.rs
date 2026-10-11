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
    assert!(out.is_empty());
    assert_eq!(
        probe
            .written
            .borrow()
            .iter()
            .filter(|b| **b == b'\n')
            .count(),
        1
    );
    Ok(())
}

#[test]
fn record_parses_as_a_flag() -> Result<()> {
    let parse = |argv: &[&str]| crate::parse_args(argv.iter().map(|arg| (*arg).to_owned()));
    assert!(parse(&["dir", "--run", "--record"])?.record);
    assert!(!parse(&["dir", "--run"])?.record);
    Ok(())
}
