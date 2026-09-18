//! Input contract and non-mutating Git validation.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{error, fmt};
use std::{ffi::OsString, os::unix::ffi::OsStringExt};

use serde::Deserialize;
use tinyhivemind_embed::RouteCandidate;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Task {
    pub(super) instance_id: String,
    pub(super) repo_path: PathBuf,
    pub(super) base_commit: String,
    pub(super) problem_statement: String,
    pub(super) test_command: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct GitLayout {
    pub(super) dot_git_is_file: bool,
    pub(super) git_dir: PathBuf,
    pub(super) common_dir: PathBuf,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum TaskValidationFailure {
    UnsupportedSubmodule { path: PathBuf },
}

impl fmt::Display for TaskValidationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSubmodule { path } => write!(
                formatter,
                "Git submodules are unsupported: {} is a gitlink",
                path.display()
            ),
        }
    }
}

impl error::Error for TaskValidationFailure {}

impl Task {
    pub(super) fn load(path: &Path) -> anyhow::Result<Self> {
        let task: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        task.validate()?;
        Ok(task)
    }

    pub(super) fn validate(&self) -> anyhow::Result<()> {
        for (name, value) in [
            ("instance_id", self.instance_id.as_str()),
            ("base_commit", self.base_commit.as_str()),
            ("problem_statement", self.problem_statement.as_str()),
            ("test_command", self.test_command.as_str()),
        ] {
            if value.trim().is_empty() {
                anyhow::bail!("task field {name} must not be blank");
            }
        }
        if !self.repo_path.is_absolute() {
            anyhow::bail!("repo_path must be absolute");
        }
        if !self.repo_path.is_dir() {
            anyhow::bail!("repo_path is not a directory: {}", self.repo_path.display());
        }
        self.git(
            ["rev-parse", "--is-inside-work-tree"],
            "validate git checkout",
        )?;
        let root = self.git(["rev-parse", "--show-toplevel"], "resolve checkout root")?;
        let canonical_repo = self.repo_path.canonicalize()?;
        if canonical_repo != Path::new(root.trim()).canonicalize()? {
            anyhow::bail!("repo_path must equal the Git checkout root");
        }
        self.git_layout()?.validate_location(&canonical_repo)?;
        let kind = self.git(
            ["cat-file", "-t", &self.base_commit],
            "validate base_commit",
        )?;
        if kind.trim() != "commit" {
            anyhow::bail!("base_commit must name a commit object directly");
        }
        let resolved = self.git(
            ["rev-parse", &format!("{}^{{commit}}", self.base_commit)],
            "resolve base_commit",
        )?;
        let head = self.git(["rev-parse", "HEAD"], "resolve HEAD")?;
        if resolved.trim() != head.trim() {
            anyhow::bail!("HEAD must equal the resolved base_commit");
        }
        let index = self.git_bytes(
            ["ls-files", "--stage", "-z"],
            "inspect checkout for gitlinks",
        )?;
        for entry in index
            .split(|byte| *byte == 0)
            .filter(|entry| !entry.is_empty())
        {
            if !entry.starts_with(b"160000 ") {
                continue;
            }
            let path = entry
                .splitn(2, |byte| *byte == b'\t')
                .nth(1)
                .map(|path| PathBuf::from(OsString::from_vec(path.to_vec())))
                .unwrap_or_else(|| PathBuf::from("<unknown>"));
            return Err(TaskValidationFailure::UnsupportedSubmodule { path }.into());
        }
        let status = self.git(["status", "--porcelain=v1"], "inspect checkout status")?;
        if !status.trim().is_empty() {
            anyhow::bail!(
                "repo_path must start clean; uncommitted changes make attribution ambiguous"
            );
        }
        let ignored = self.git(
            ["ls-files", "--others", "--ignored", "--exclude-standard"],
            "inspect ignored checkout files",
        )?;
        if !ignored.trim().is_empty() {
            anyhow::bail!("repo_path must start clean; ignored files are not allowed");
        }
        Ok(())
    }

    pub(super) fn candidate(&self, id: &str) -> RouteCandidate {
        RouteCandidate {
            id: id.into(),
            label: id.into(),
            role: Some(id.into()),
            description: Some(format!("{id} seat for software-engineering task")),
            capabilities: vec!["local repository work".into()],
            learned_topics: Vec::new(),
            available: true,
        }
    }

    fn git<const N: usize>(&self, args: [&str; N], action: &str) -> anyhow::Result<String> {
        Ok(String::from_utf8(self.git_bytes(args, action)?)?)
    }

    fn git_bytes<const N: usize>(&self, args: [&str; N], action: &str) -> anyhow::Result<Vec<u8>> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.repo_path)
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/local/bin")
            .output()?;
        if !output.status.success() {
            anyhow::bail!(
                "{action} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(output.stdout)
    }

    pub(super) fn git_layout(&self) -> anyhow::Result<GitLayout> {
        GitLayout::discover(&self.repo_path)
    }
}

impl GitLayout {
    pub(super) fn discover(repo_path: &Path) -> anyhow::Result<Self> {
        let dot_git = repo_path.join(".git");
        let metadata = std::fs::symlink_metadata(&dot_git)?;
        if metadata.file_type().is_symlink() {
            anyhow::bail!("repo_path/.git must not be a symlink");
        }
        let dot_git_is_file = if metadata.is_file() {
            true
        } else if metadata.is_dir() {
            false
        } else {
            anyhow::bail!("repo_path/.git must be a regular file or directory");
        };
        let git_dir = git(
            repo_path,
            ["rev-parse", "--absolute-git-dir"],
            "resolve Git directory",
        )?;
        let git_dir = PathBuf::from(git_dir.trim()).canonicalize()?;
        let common = PathBuf::from(
            git(
                repo_path,
                ["rev-parse", "--git-common-dir"],
                "resolve common Git directory",
            )?
            .trim(),
        );
        let common_dir = if common.is_absolute() {
            common
        } else {
            repo_path.join(common)
        }
        .canonicalize()?;
        let layout = GitLayout {
            dot_git_is_file,
            git_dir,
            common_dir,
        };
        layout.validate_location(&repo_path.canonicalize()?)?;
        Ok(layout)
    }

    fn validate_location(&self, canonical_repo: &Path) -> anyhow::Result<()> {
        let dot_git = canonical_repo.join(".git");
        let metadata = std::fs::symlink_metadata(&dot_git)?;
        if metadata.file_type().is_symlink() {
            anyhow::bail!("repo_path/.git must not be a symlink");
        }
        if self.dot_git_is_file {
            if !metadata.is_file() {
                anyhow::bail!("linked-worktree .git must be a regular file");
            }
            if self.git_dir.starts_with(canonical_repo)
                || self.common_dir.starts_with(canonical_repo)
            {
                anyhow::bail!("linked-worktree Git metadata must be outside repo_path");
            }
            if self.git_dir.parent().and_then(Path::parent) != Some(self.common_dir.as_path())
                || self.git_dir.parent().and_then(Path::file_name)
                    != Some(std::ffi::OsStr::new("worktrees"))
            {
                anyhow::bail!(".git file must describe a linked worktree");
            }
        } else {
            if !metadata.is_dir() {
                anyhow::bail!("standard .git must be a real directory");
            }
            let standard = dot_git.canonicalize()?;
            if self.git_dir != standard || self.common_dir != standard {
                anyhow::bail!("Git metadata must be exactly the standard repo_path/.git directory");
            }
        }
        Ok(())
    }
}

fn git<const N: usize>(repo_path: &Path, args: [&str; N], action: &str) -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/local/bin")
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "{action} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?)
}
