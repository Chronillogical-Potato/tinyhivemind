//! Fresh, outside-checkout paths for one DeepSWE run's durable artifacts.

use std::path::{Path, PathBuf};

use super::Task;

#[derive(Debug)]
pub(super) struct ArtifactPaths {
    pub(super) transcript: PathBuf,
    pub(super) outbox: PathBuf,
    pub(super) runtime: PathBuf,
}

pub(super) fn validate_output_paths(task: &Task, output: &Path) -> anyhow::Result<()> {
    if !output.is_absolute() {
        anyhow::bail!("--output must be absolute");
    }
    if output.components().any(|component| {
        matches!(
            component,
            std::path::Component::CurDir | std::path::Component::ParentDir
        )
    }) {
        anyhow::bail!("--output must not contain . or .. components");
    }
    let repo = task.repo_path.canonicalize()?;
    for (name, path) in [
        ("output", output.to_path_buf()),
        ("transcript", output_sibling(output, "transcript.md")?),
        ("outbox", output_sibling(output, "outboxes")?),
        ("runtime", output_sibling(output, "runtime")?),
    ] {
        let resolved = canonicalize_destination(&path)?;
        if resolved.starts_with(&repo) {
            anyhow::bail!("{name} path must be outside canonical repo_path");
        }
    }
    Ok(())
}

pub(super) fn prepare_artifacts(task: &Task, output: &Path) -> anyhow::Result<ArtifactPaths> {
    validate_output_paths(task, output)?;
    let artifacts = ArtifactPaths {
        transcript: output_sibling(output, "transcript.md")?,
        outbox: output_sibling(output, "outboxes")?,
        runtime: output_sibling(output, "runtime")?,
    };
    for (name, path) in [
        ("result", output),
        ("transcript", artifacts.transcript.as_path()),
        ("outbox", artifacts.outbox.as_path()),
        ("runtime", artifacts.runtime.as_path()),
    ] {
        match std::fs::symlink_metadata(path) {
            Ok(_) => anyhow::bail!("{name} artifact already exists: {}", path.display()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    std::fs::create_dir(&artifacts.runtime)?;
    if let Err(error) = std::fs::create_dir(&artifacts.outbox) {
        let _ = std::fs::remove_dir(&artifacts.runtime);
        return Err(error.into());
    }
    Ok(artifacts)
}

fn canonicalize_destination(path: &Path) -> anyhow::Result<PathBuf> {
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::CurDir | std::path::Component::ParentDir
        )
    }) {
        anyhow::bail!("output path must not contain . or .. components");
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            anyhow::bail!("output destination must not be a symlink")
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("output path has no parent"))?;
    if !parent.is_dir() {
        anyhow::bail!("output parent must already exist and be a directory");
    }
    let name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("output path must end in a normal filename"))?;
    let resolved_parent = parent.canonicalize()?;
    if path.try_exists()? {
        let resolved = path.canonicalize()?;
        if resolved.parent() != Some(resolved_parent.as_path()) {
            return Ok(resolved);
        }
    }
    Ok(resolved_parent.join(name))
}

pub(super) fn output_sibling(output: &Path, name: &str) -> anyhow::Result<PathBuf> {
    Ok(output
        .parent()
        .ok_or_else(|| anyhow::anyhow!("output path has no parent"))?
        .join(name))
}
