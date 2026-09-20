//! Regression tests for hostile paths and bounded Docker process handling.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use tempfile::TempDir;

use super::super::sandbox::{
    DockerSandbox, InspectorFailure, MAX_ACTION_INPUT_BYTES, SandboxConfig, patch_with_limits_at,
    preflight_with_limits, shell_with_limits, shell_with_limits_at,
};
use super::super::task::TaskValidationFailure;
use super::super::{Task, prepare_artifacts, validate_output_paths};
use super::real_docker_test_guard;

#[test]
fn rejects_a_symlinked_dot_git_directory() {
    let directory = TempDir::new().expect("temporary repository");
    git(directory.path(), &["init", "-q"]);
    git(
        directory.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(
        directory.path(),
        &["config", "user.name", "DeepSWE Fixture"],
    );
    std::fs::write(directory.path().join("answer.txt"), "wrong\n").expect("fixture source");
    git(directory.path(), &["add", "."]);
    git(directory.path(), &["commit", "-qm", "fixture"]);
    std::fs::rename(
        directory.path().join(".git"),
        directory.path().join(".realgit"),
    )
    .expect("move metadata");
    std::os::unix::fs::symlink(".realgit", directory.path().join(".git"))
        .expect("symlink metadata");

    let task = task_for(directory.path());
    assert!(
        task.validate()
            .expect_err("symlinked metadata rejected")
            .to_string()
            .contains("symlink")
    );
}

#[test]
fn rejects_ambiguous_or_missing_output_parents() {
    let directory = TempDir::new().expect("temporary repository");
    git(directory.path(), &["init", "-q"]);
    git(
        directory.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(
        directory.path(),
        &["config", "user.name", "DeepSWE Fixture"],
    );
    std::fs::write(directory.path().join("answer.txt"), "wrong\n").expect("fixture source");
    git(directory.path(), &["add", "."]);
    git(directory.path(), &["commit", "-qm", "fixture"]);
    let task = task_for(directory.path());
    let parent = directory.path().parent().expect("temporary parent");

    let ambiguous = parent
        .join("missing-output-parent")
        .join("..")
        .join(directory.path().file_name().expect("repository name"))
        .join("result.json");
    assert!(validate_output_paths(&task, &ambiguous).is_err());
    assert!(
        validate_output_paths(&task, &parent.join("missing-output-parent/result.json"))
            .expect_err("missing parent rejected")
            .to_string()
            .contains("parent")
    );
}

#[test]
fn rejects_a_broken_symlink_at_every_output_destination() {
    let (directory, task) = repository_fixture();
    let output_parent = TempDir::new().expect("output parent");
    let output = output_parent.path().join("result.json");

    for target in ["result.json", "transcript.md", "outboxes", "runtime"] {
        let symlink = output_parent.path().join(target);
        std::os::unix::fs::symlink(directory.path().join("future-result"), &symlink)
            .expect("broken symlink");
        let error = validate_output_paths(&task, &output).expect_err("symlink rejected");
        assert!(error.to_string().contains("symlink"), "{error:#}");
        std::fs::remove_file(symlink).expect("remove broken symlink");
    }
}

#[test]
fn every_deepswe_artifact_target_must_be_absent() {
    let (_directory, task) = repository_fixture();
    for target in ["result.json", "transcript.md", "outboxes", "runtime"] {
        let output_parent = TempDir::new().expect("output parent");
        let output = output_parent.path().join("result.json");
        let path = output_parent.path().join(target);
        if matches!(target, "outboxes" | "runtime") {
            std::fs::create_dir(&path).expect("stale directory");
        } else {
            std::fs::write(&path, "stale").expect("stale file");
        }
        let error = prepare_artifacts(&task, &output).expect_err("stale artifact rejected");
        assert!(error.to_string().contains("already exists"), "{error:#}");
        assert!(!output_parent.path().join("provider-started").exists());
        assert!(!output_parent.path().join("docker-started").exists());
    }
}

#[test]
fn preexisting_outbox_directory_is_rejected_before_external_setup() {
    let (_directory, task) = repository_fixture();
    let output_parent = TempDir::new().expect("output parent");
    let output = output_parent.path().join("result.json");
    std::fs::create_dir(output_parent.path().join("outboxes")).expect("outbox");
    let error = prepare_artifacts(&task, &output).expect_err("stale outbox rejected");
    assert!(error.to_string().contains("already exists"), "{error:#}");
}

#[test]
fn outbox_initialization_truncates_regular_files_and_rejects_hostile_children() {
    let directory = TempDir::new().expect("outbox directory");
    let child = directory.path().join("lead.jsonl");
    std::fs::write(&child, "stale action\n").expect("regular child");
    super::super::mcp::clear(&child).expect("regular child truncates");
    assert!(
        std::fs::read_to_string(&child)
            .expect("read child")
            .is_empty()
    );
    std::fs::remove_file(&child).expect("remove child");
    std::os::unix::fs::symlink(directory.path().join("missing"), &child).expect("outbox symlink");
    let error = super::super::mcp::clear(&child).expect_err("hostile child rejected");
    assert!(error.to_string().contains("regular file"), "{error:#}");
}

#[test]
fn preflight_hangs_are_bounded_and_created_containers_are_removed() {
    let _guard = preflight_test_guard();
    assert_preflight_failures_are_bounded("hang");
}

#[test]
fn preflight_output_is_bounded_and_created_containers_are_removed() {
    let _guard = preflight_test_guard();
    assert_preflight_failures_are_bounded("overflow");
}

#[test]
fn rejects_a_gitlink_before_sandbox_or_provider_work() {
    let (directory, mut task) = repository_fixture();
    git(
        directory.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{},vendor/dependency", task.base_commit),
        ],
    );
    git(directory.path(), &["commit", "-qm", "add gitlink"]);
    task.base_commit = git_output(directory.path(), &["rev-parse", "HEAD"])
        .trim()
        .into();

    let error = task.validate().expect_err("gitlink rejected");
    assert!(matches!(
        error.downcast_ref::<TaskValidationFailure>(),
        Some(TaskValidationFailure::UnsupportedSubmodule { path })
            if path == Path::new("vendor/dependency")
    ));
}

#[test]
fn rejects_oversized_action_input_without_starting_docker() {
    let (directory, task) = repository_fixture();
    let marker = directory.path().join("action-started");
    let removed = directory.path().join("preflight-removed");
    let docker = directory.path().join("fake-docker-input-cap");
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\nversion) printf fixture ;;\ncreate) rm -f '{}'; printf abcdef1234567890 ;;\ninspect) if test -e '{}'; then printf 'Error: No such object: %s' \"$2\" >&2; exit 1; fi; printf '[{{\"HostConfig\":{{\"NetworkMode\":\"none\"}},\"Mounts\":[{{\"Destination\":\"/workspace\",\"RW\":true}},{{\"Destination\":\"/workspace/.git\",\"RW\":false}}]}}]' ;;\nstart) printf sandbox-ready ;;\nrm) touch '{}'; exit 0 ;;\nrun) touch '{}'; exit 0 ;;\n*) exit 1 ;;\nesac\n",
        removed.display(),
        removed.display(),
        removed.display(),
        marker.display(),
    );
    std::fs::write(&docker, script).expect("fake Docker");
    executable(&docker);
    let sandbox = DockerSandbox::preflight(SandboxConfig {
        repo_path: task.repo_path.clone(),
        image: "local-fixture".into(),
        docker,
    })
    .expect("preflight");

    let content = "x".repeat(MAX_ACTION_INPUT_BYTES + 1);
    let started = std::time::Instant::now();
    let error = sandbox
        .file_write("large.txt", &content)
        .expect_err("oversized input rejected");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(
        error.downcast_ref::<InspectorFailure>(),
        Some(&InspectorFailure::ActionInputTooLarge {
            max_bytes: MAX_ACTION_INPUT_BYTES
        })
    );
    assert!(!marker.exists(), "Docker action must not be spawned");
}

#[test]
fn timeout_removes_the_started_container() {
    let (directory, task) = repository_fixture();
    let marker = directory.path().join("removed-container");
    let docker = directory.path().join("fake-docker-timeout-cleanup");
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\nversion) printf fixture ;;\ncreate) rm -f '{}'; printf abcdef1234567890 ;;\ninspect) if test -e '{}'; then printf 'Error: No such object: %s' \"$2\" >&2; exit 1; fi; printf '[{{\"HostConfig\":{{\"NetworkMode\":\"none\"}},\"Mounts\":[{{\"Destination\":\"/workspace\",\"RW\":true}},{{\"Destination\":\"/workspace/.git\",\"RW\":false}}]}}]' ;;\nstart) printf sandbox-ready ;;\nrm) touch '{}' ;;\nrun) shift; while test \"$#\" -gt 0; do if test \"$1\" = --cidfile; then shift; printf abcdef1234567890 > \"$1\"; break; fi; shift; done; exec sleep 10 ;;\n*) exit 1 ;;\nesac\n",
        marker.display(),
        marker.display(),
        marker.display()
    );
    std::fs::write(&docker, script).expect("fake Docker");
    executable(&docker);
    let sandbox = DockerSandbox::preflight(SandboxConfig {
        repo_path: task.repo_path.clone(),
        image: "local-fixture".into(),
        docker,
    })
    .expect("preflight");
    std::fs::remove_file(&marker).expect("discard preflight cleanup marker");

    let error = shell_with_limits(&sandbox, "true", Duration::from_millis(50), 128)
        .expect_err("hung action rejected");
    assert!(matches!(
        error.downcast_ref::<InspectorFailure>(),
        Some(InspectorFailure::TimedOut { .. })
    ));
    assert!(marker.exists(), "timed-out container must be removed");
}

#[test]
fn cleanup_failure_is_typed_and_propagated() {
    let (directory, task) = repository_fixture();
    let docker = directory.path().join("fake-docker-cleanup-failure");
    let state = directory.path().join("cleanup-failure-container");
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\nversion) printf fixture ;;\ncreate) touch '{}'; printf abcdef1234567890 ;;\ninspect)\n  test -e '{}' || exit 1\n  printf '[{{\"HostConfig\":{{\"NetworkMode\":\"none\"}},\"Mounts\":[{{\"Destination\":\"/workspace\",\"RW\":true}},{{\"Destination\":\"/workspace/.git\",\"RW\":false}}]}}]' ;;\nstart) printf sandbox-ready ;;\nrm) printf cleanup-failed >&2; exit 23 ;;\n*) exit 1 ;;\nesac\n",
        state.display(),
        state.display(),
    );
    std::fs::write(&docker, script).expect("fake Docker");
    executable(&docker);

    let error = DockerSandbox::preflight(SandboxConfig {
        repo_path: task.repo_path.clone(),
        image: "local-fixture".into(),
        docker,
    })
    .expect_err("cleanup failure propagated");
    assert!(matches!(
        error.downcast_ref::<InspectorFailure>(),
        Some(InspectorFailure::CleanupFailed { .. })
    ));
}

#[test]
fn cleanup_does_not_treat_daemon_errors_as_absence() {
    let (directory, task) = repository_fixture();
    let docker = directory.path().join("fake-docker-cleanup-daemon-error");
    let state = directory.path().join("cleanup-daemon-error-container");
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\nversion) printf fixture ;;\ncreate) touch '{}'; printf abcdef1234567890 ;;\ninspect)\n  if test -e '{}'; then\n    printf '[{{\"HostConfig\":{{\"NetworkMode\":\"none\"}},\"Mounts\":[{{\"Destination\":\"/workspace\",\"RW\":true}},{{\"Destination\":\"/workspace/.git\",\"RW\":false}}]}}]'\n  else\n    printf 'Cannot connect to the Docker daemon' >&2; exit 1\n  fi ;;\nstart) printf sandbox-ready ;;\nrm) rm -f '{}'; printf 'Cannot connect to the Docker daemon' >&2; exit 23 ;;\n*) exit 1 ;;\nesac\n",
        state.display(),
        state.display(),
        state.display(),
    );
    std::fs::write(&docker, script).expect("fake Docker");
    executable(&docker);

    let error = DockerSandbox::preflight(SandboxConfig {
        repo_path: task.repo_path.clone(),
        image: "local-fixture".into(),
        docker,
    })
    .expect_err("daemon errors do not prove absence");
    assert!(matches!(
        error.downcast_ref::<InspectorFailure>(),
        Some(InspectorFailure::CleanupFailed { detail })
            if detail.contains("Cannot connect to the Docker daemon")
    ));
}

#[test]
fn cleanup_accepts_exact_no_such_object_after_failed_removal() {
    let (directory, task) = repository_fixture();
    let docker = directory.path().join("fake-docker-cleanup-not-found");
    let state = directory.path().join("cleanup-not-found-container");
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\nversion) printf fixture ;;\ncreate) touch '{}'; printf abcdef1234567890 ;;\ninspect)\n  if test -e '{}'; then\n    printf '[{{\"HostConfig\":{{\"NetworkMode\":\"none\"}},\"Mounts\":[{{\"Destination\":\"/workspace\",\"RW\":true}},{{\"Destination\":\"/workspace/.git\",\"RW\":false}}]}}]'\n  else\n    printf 'Error: No such object: %s' \"$2\" >&2; exit 1\n  fi ;;\nstart) printf sandbox-ready ;;\nrm) rm -f '{}'; printf cleanup-failed >&2; exit 23 ;;\n*) exit 1 ;;\nesac\n",
        state.display(),
        state.display(),
        state.display(),
    );
    std::fs::write(&docker, script).expect("fake Docker");
    executable(&docker);

    DockerSandbox::preflight(SandboxConfig {
        repo_path: task.repo_path.clone(),
        image: "local-fixture".into(),
        docker,
    })
    .expect("exact no-such-object proves absence");
}

#[test]
fn cleanup_rejects_not_found_text_in_an_unrelated_diagnostic() {
    let (directory, task) = repository_fixture();
    let docker = directory
        .path()
        .join("fake-docker-cleanup-misleading-not-found");
    let state = directory.path().join("cleanup-misleading-container");
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\nversion) printf fixture ;;\ncreate) touch '{}'; printf abcdef1234567890 ;;\ninspect)\n  if test -e '{}'; then\n    printf '[{{\"HostConfig\":{{\"NetworkMode\":\"none\"}},\"Mounts\":[{{\"Destination\":\"/workspace\",\"RW\":true}},{{\"Destination\":\"/workspace/.git\",\"RW\":false}}]}}]'\n  else\n    printf 'daemon unavailable while reporting Error: No such object: %s' \"$2\" >&2; exit 1\n  fi ;;\nstart) printf sandbox-ready ;;\nrm) rm -f '{}'; printf cleanup-failed >&2; exit 23 ;;\n*) exit 1 ;;\nesac\n",
        state.display(),
        state.display(),
        state.display(),
    );
    std::fs::write(&docker, script).expect("fake Docker");
    executable(&docker);

    let error = DockerSandbox::preflight(SandboxConfig {
        repo_path: task.repo_path.clone(),
        image: "local-fixture".into(),
        docker,
    })
    .expect_err("unrelated diagnostic does not prove absence");
    assert!(matches!(
        error.downcast_ref::<InspectorFailure>(),
        Some(InspectorFailure::CleanupFailed { .. })
    ));
}

#[test]
fn cleanup_timeout_is_bounded_and_typed() {
    let (directory, task) = repository_fixture();
    let docker = directory.path().join("fake-docker-cleanup-timeout");
    let script = "#!/bin/sh\ncase \"$1\" in\nversion) printf fixture ;;\ncreate) printf abcdef1234567890 ;;\ninspect) printf '[{\"HostConfig\":{\"NetworkMode\":\"none\"},\"Mounts\":[{\"Destination\":\"/workspace\",\"RW\":true},{\"Destination\":\"/workspace/.git\",\"RW\":false}]}]' ;;\nstart) printf sandbox-ready ;;\nrm) exec sleep 30 ;;\n*) exit 1 ;;\nesac\n";
    std::fs::write(&docker, script).expect("fake Docker");
    executable(&docker);

    let started = std::time::Instant::now();
    let error = DockerSandbox::preflight(SandboxConfig {
        repo_path: task.repo_path.clone(),
        image: "local-fixture".into(),
        docker,
    })
    .expect_err("cleanup timeout propagated");
    assert!(started.elapsed() < Duration::from_secs(15));
    assert_eq!(
        error.downcast_ref::<InspectorFailure>(),
        Some(&InspectorFailure::CleanupTimeout { seconds: 2 })
    );
}

#[test]
fn live_real_docker_caps_action_and_patch_files_before_host_buffering() {
    if std::env::var_os("DEEPSWE_REAL_DOCKER_TEST").is_none() {
        return;
    }
    let _docker_guard = real_docker_test_guard();
    let (directory, task) = repository_fixture();
    let sandbox = DockerSandbox::preflight(SandboxConfig::from_env(task.repo_path.clone()))
        .expect("real Docker preflight");
    let image = std::env::var("DEEPSWE_DOCKER_IMAGE")
        .unwrap_or_else(|_| "tinyhivemind-deepswe:local".into());
    let before = running_containers(&image);

    let action_file = directory.path().join("bounded-action-output");
    std::fs::write(&action_file, []).expect("action output mount");
    let action = shell_with_limits_at(&sandbox, "yes", Duration::from_secs(10), 128, &action_file)
        .expect_err("unbounded action rejected");
    assert_eq!(
        action.downcast_ref::<InspectorFailure>(),
        Some(&InspectorFailure::ActionOutputTooLarge { max_bytes: 128 })
    );
    assert!(
        std::fs::metadata(action_file)
            .expect("action metadata")
            .len()
            <= 129
    );

    sandbox
        .shell("truncate -s 1048576 huge-sparse.bin")
        .expect("large sparse file");
    let patch_file = directory.path().join("bounded.patch");
    std::fs::write(&patch_file, []).expect("patch mount");
    let patch = patch_with_limits_at(
        &sandbox,
        &task.base_commit,
        Duration::from_secs(10),
        128,
        &patch_file,
    )
    .expect_err("oversized patch rejected");
    assert_eq!(
        patch.downcast_ref::<InspectorFailure>(),
        Some(&InspectorFailure::PatchTooLarge { max_bytes: 128 })
    );
    assert!(std::fs::metadata(patch_file).expect("patch metadata").len() <= 129);
    assert_eq!(running_containers(&image), before);
}

#[test]
fn live_real_docker_preflight_leaves_no_named_container() {
    if std::env::var_os("DEEPSWE_REAL_DOCKER_TEST").is_none() {
        return;
    }
    let _docker_guard = real_docker_test_guard();
    let (_directory, task) = repository_fixture();
    let before = preflight_containers();
    DockerSandbox::preflight(SandboxConfig::from_env(task.repo_path))
        .expect("real Docker preflight");
    assert_eq!(preflight_containers(), before);
}

fn assert_preflight_failures_are_bounded(mode: &str) {
    for phase in ["version", "create", "inspect", "start"] {
        let (directory, task) = repository_fixture();
        let state = directory.path().join("container-exists");
        let removed = directory.path().join("container-removed");
        let docker = directory.path().join(format!("fake-docker-{phase}-{mode}"));
        let behavior = match mode {
            "hang" => "exec sleep 30",
            "overflow" => "yes x | head -c 10000; exit 0",
            _ => unreachable!("test mode"),
        };
        let script = format!(
            r#"#!/bin/sh
phase='{phase}'
behavior() {{ {behavior}; }}
case "$1" in
version)
  test "$phase" = version && behavior
  printf fixture
  ;;
create)
  cidfile=
  shift
  while test "$#" -gt 0; do
    if test "$1" = --cidfile; then shift; cidfile=$1; fi
    shift
  done
  printf abcdef1234567890 > "$cidfile"
  touch '{state}'
  test "$phase" = create && behavior
  printf abcdef1234567890
  ;;
inspect)
  test -e '{state}' || exit 1
  test "$phase" = inspect && behavior
  printf '[{{"HostConfig":{{"NetworkMode":"none"}},"Mounts":[{{"Destination":"/workspace","RW":true}},{{"Destination":"/workspace/.git","RW":false}}]}}]'
  ;;
start)
  test "$phase" = start && behavior
  printf sandbox-ready
  ;;
rm)
  rm -f '{state}'
  touch '{removed}'
  ;;
*) exit 1 ;;
esac
"#,
            state = state.display(),
            removed = removed.display(),
        );
        std::fs::write(&docker, script).expect("fake Docker");
        executable(&docker);
        let started = std::time::Instant::now();
        let error = preflight_with_limits(
            SandboxConfig {
                repo_path: task.repo_path,
                image: "local-fixture".into(),
                docker,
            },
            Duration::from_secs(5),
            128,
        )
        .expect_err("unbounded preflight rejected");
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "{phase}: {error:#}"
        );
        if phase == "version" {
            assert!(!removed.exists(), "version creates no container");
        } else {
            assert!(
                removed.exists(),
                "{phase} container was not removed: {error:#}"
            );
            assert!(!state.exists(), "{phase} container still exists");
        }
    }
}

fn preflight_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static PREFLIGHT: std::sync::Mutex<()> = std::sync::Mutex::new(());
    PREFLIGHT.lock().expect("preflight test lock")
}

fn repository_fixture() -> (TempDir, Task) {
    let directory = TempDir::new().expect("temporary repository");
    git(directory.path(), &["init", "-q"]);
    git(
        directory.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(
        directory.path(),
        &["config", "user.name", "DeepSWE Fixture"],
    );
    std::fs::write(directory.path().join("answer.txt"), "wrong\n").expect("fixture source");
    git(directory.path(), &["add", "."]);
    git(directory.path(), &["commit", "-qm", "fixture"]);
    let task = task_for(directory.path());
    (directory, task)
}

fn task_for(repository: &Path) -> Task {
    Task {
        instance_id: "security-fixture".into(),
        repo_path: repository.to_path_buf(),
        base_commit: git_output(repository, &["rev-parse", "HEAD"]).trim().into(),
        problem_statement: "change the answer".into(),
        test_command: "true".into(),
    }
}

fn executable(path: &Path) {
    let mut permissions = std::fs::metadata(path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("executable");
}

fn git(directory: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .args(args)
            .current_dir(directory)
            .status()
            .expect("git runs")
            .success()
    );
}

fn git_output(directory: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(directory)
        .output()
        .expect("git runs");
    assert!(output.status.success());
    String::from_utf8(output.stdout).expect("utf8 git output")
}

fn running_containers(image: &str) -> Vec<String> {
    let output = Command::new("docker")
        .args(["ps", "--quiet", "--filter", &format!("ancestor={image}")])
        .output()
        .expect("docker ps");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .expect("utf8 container ids")
        .lines()
        .map(str::to_owned)
        .collect()
}

fn preflight_containers() -> Vec<String> {
    let output = Command::new("docker")
        .args([
            "ps",
            "--all",
            "--quiet",
            "--filter",
            "name=deepswe-preflight-",
        ])
        .output()
        .expect("docker ps");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .expect("utf8 container ids")
        .lines()
        .map(str::to_owned)
        .collect()
}
