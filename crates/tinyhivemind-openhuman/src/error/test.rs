//! Every variant says what failed, in the words a host prints.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use super::Error;

#[test]
fn each_failure_names_itself() {
    let cases: Vec<(Error, &str)> = vec![
        (Error::UnknownRunner("rae".into()), "TINYHIVEMIND_RUNNER"),
        (Error::IncompleteRoute, "endpoint"),
        (Error::RegistryMissing, "registry"),
        (
            Error::SeatNotRegistered {
                seat: "lead".into(),
            },
            "lead",
        ),
        (
            Error::TimedOut {
                seat: "lead".into(),
            },
            "@lead timed out",
        ),
        (
            Error::Io(std::io::Error::other("disk")),
            "writing a seat definition",
        ),
        (Error::Harness(anyhow::anyhow!("refused")), "refused"),
    ];
    for (error, expected) in cases {
        assert!(
            error.to_string().contains(expected),
            "{error}: expected {expected:?}"
        );
    }
}
