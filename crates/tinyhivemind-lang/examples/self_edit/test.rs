//! The self-edit example runs offline and keeps the rejected design out of
//! the accepted head.
use super::{Result, run};

#[test]
fn refuses_the_tampered_edit_and_activates_only_the_accepted_one() -> Result<()> {
    let summary = run()?;
    assert_eq!(
        summary.refusal,
        ["constitution.json:/max_turns: immutable: the constitution is host-owned and no edit \
          may change it"]
    );
    assert_eq!(summary.archived, 3);
    assert_eq!(summary.frontier, 1);
    assert_eq!(summary.head, summary.accepted_id);
    super::main()
}
