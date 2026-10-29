//! Project files (`*.legolab`, JSON).

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::camera::pose::{self, PoseClass};
use crate::visual::Workspace;

pub const FORMAT_VERSION: u32 = 1;
pub const EXTENSION: &str = "legolab";
/// Refuse absurdly large files instead of trying to parse them.
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub name: String,
    #[serde(flatten)]
    pub workspace: Workspace,
    /// Camera poses the project has been taught.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub poses: Vec<PoseClass>,
}

impl Project {
    pub fn new(name: impl Into<String>, workspace: Workspace) -> Self {
        Self { version: FORMAT_VERSION, name: name.into(), workspace, poses: Vec::new() }
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Parse and validate. Every parameter is clamped and the block
    /// structure repaired, so a malformed file cannot produce arbitrary
    /// hardware commands.
    pub fn from_json(text: &str) -> Result<Self> {
        let mut project: Project =
            serde_json::from_str(text).context("this is not a valid LEGO Lab project")?;
        if project.version > FORMAT_VERSION {
            bail!(
                "this project was made with a newer version of LEGO Lab (format {})",
                project.version
            );
        }
        project.version = FORMAT_VERSION;
        project.workspace.sanitize();
        pose::sanitize_classes(&mut project.poses);
        Ok(project)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let size = std::fs::metadata(path)
            .with_context(|| format!("cannot open {}", path.display()))?
            .len();
        if size > MAX_FILE_BYTES {
            bail!("file is too large to be a LEGO Lab project");
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        Self::from_json(&text)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("legolab.tmp");
        std::fs::write(&tmp, self.to_json()?)
            .with_context(|| format!("cannot write {}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .with_context(|| format!("cannot write {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut p = Project::new("My Robot", Workspace::example());
        p.workspace.sanitize();
        let back = Project::from_json(&p.to_json().unwrap()).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn trained_poses_round_trip() {
        let mut p = Project::new("Dance", Workspace::example());
        p.workspace.sanitize();
        let mut wave = PoseClass::new("wave");
        wave.examples.push(crate::camera::pose::tests::standing(true, false));
        p.poses.push(wave);
        let back = Project::from_json(&p.to_json().unwrap()).unwrap();
        assert_eq!(back.poses.len(), 1);
        assert_eq!(back.poses[0].name, "wave");
        assert_eq!(back.poses[0].examples[0].builtin(), Some(crate::camera::CameraPose::LeftArmUp));
    }

    #[test]
    fn rejects_unknown_block_types_and_future_versions() {
        let bad = r#"{"version":1,"name":"x","stacks":[{"pos":[0,0],"blocks":[{"id":1,"type":"self_destruct"}]}]}"#;
        assert!(Project::from_json(bad).is_err());
        let future = r#"{"version":99,"name":"x","stacks":[]}"#;
        assert!(Project::from_json(future).is_err());
    }
}
