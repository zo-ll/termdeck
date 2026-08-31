//! YAML configuration loading and validation.

use std::{
    collections::BTreeMap,
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::contracts::{Project, TerminalId};

const MIN_MASTER_RATIO: f64 = 0.55;
const MAX_MASTER_RATIO: f64 = 0.85;

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub workspaces: BTreeMap<String, Workspace>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Workspace {
    pub name: String,
    pub root: PathBuf,
    pub projects: Vec<Project>,
    pub scrollback: usize,
    pub master_ratio: MasterRatio,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MasterRatio(f64);

impl MasterRatio {
    pub const fn get(self) -> f64 {
        self.0
    }
}

#[derive(Debug)]
pub struct ConfigError {
    message: String,
}

impl ConfigError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl Error for ConfigError {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u8,
    defaults: RawDefaults,
    workspaces: BTreeMap<String, RawWorkspace>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDefaults {
    command: Vec<String>,
    #[serde(default = "default_scrollback")]
    scrollback: usize,
    #[serde(default = "default_master_ratio")]
    master_ratio: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkspace {
    root: PathBuf,
    terminals: Vec<RawProject>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProject {
    name: String,
    #[serde(alias = "path")]
    cwd: PathBuf,
    command: Option<Vec<String>>,
    #[serde(default)]
    optional: bool,
}

const fn default_scrollback() -> usize {
    10_000
}

const fn default_master_ratio() -> f64 {
    0.70
}

/// Loads and validates a Termdeck YAML configuration file.
pub fn load(path: impl AsRef<Path>) -> Result<Config, ConfigError> {
    let path = path.as_ref();
    let source = fs::read_to_string(path)
        .map_err(|error| ConfigError::new(format!("cannot read {}: {error}", path.display())))?;
    let raw: RawConfig = serde_yaml::from_str(&source).map_err(|error| {
        ConfigError::new(format!("invalid YAML in {}: {error}", path.display()))
    })?;
    validate(raw, path)
}

fn validate(raw: RawConfig, source: &Path) -> Result<Config, ConfigError> {
    if raw.version != 1 {
        return Err(ConfigError::new(format!(
            "{}: unsupported version {}; expected version: 1",
            source.display(),
            raw.version
        )));
    }
    if raw.workspaces.is_empty() {
        return Err(ConfigError::new(format!(
            "{}: add at least one workspace under workspaces",
            source.display()
        )));
    }
    validate_command(&raw.defaults.command, "defaults.command", source)?;
    if raw.defaults.scrollback == 0 {
        return Err(ConfigError::new(format!(
            "{}: defaults.scrollback must be greater than zero",
            source.display()
        )));
    }
    if !(MIN_MASTER_RATIO..=MAX_MASTER_RATIO).contains(&raw.defaults.master_ratio) {
        return Err(ConfigError::new(format!(
            "{}: defaults.master_ratio must be between {MIN_MASTER_RATIO:.2} and {MAX_MASTER_RATIO:.2}",
            source.display()
        )));
    }

    let mut workspaces = BTreeMap::new();
    for (name, workspace) in raw.workspaces {
        if name.trim().is_empty() {
            return Err(ConfigError::new(format!(
                "{}: workspace names cannot be empty",
                source.display()
            )));
        }
        if !workspace.root.is_dir() {
            return Err(ConfigError::new(format!(
                "{}: workspace '{name}' root does not exist or is not a directory: {}",
                source.display(),
                workspace.root.display()
            )));
        }
        if workspace.terminals.is_empty() || workspace.terminals.len() > 4 {
            return Err(ConfigError::new(format!(
                "{}: workspace '{name}' must define between 1 and 4 terminals",
                source.display()
            )));
        }

        let mut projects = Vec::new();
        for project in workspace.terminals {
            if project.name.trim().is_empty() {
                return Err(ConfigError::new(format!(
                    "{}: workspace '{name}' has a terminal with an empty name",
                    source.display()
                )));
            }
            if projects
                .iter()
                .any(|item: &Project| item.terminal.to_string() == project.name)
            {
                return Err(ConfigError::new(format!(
                    "{}: workspace '{name}' repeats terminal name '{}'",
                    source.display(),
                    project.name
                )));
            }

            let path = if project.cwd.is_absolute() {
                project.cwd
            } else {
                workspace.root.join(project.cwd)
            };
            if !path.is_dir() {
                if project.optional {
                    continue;
                }
                return Err(ConfigError::new(format!(
                    "{}: workspace '{name}' terminal '{}' path does not exist or is not a directory: {}",
                    source.display(),
                    project.name,
                    path.display()
                )));
            }
            let command = project
                .command
                .unwrap_or_else(|| raw.defaults.command.clone());
            validate_command(
                &command,
                &format!("workspaces.{name}.terminals.{}.command", project.name),
                source,
            )?;
            projects.push(Project {
                terminal: TerminalId::new(project.name),
                path,
                command,
            });
        }
        if projects.is_empty() {
            return Err(ConfigError::new(format!(
                "{}: workspace '{name}' has no available terminal paths",
                source.display()
            )));
        }
        workspaces.insert(
            name.clone(),
            Workspace {
                name,
                root: workspace.root,
                projects,
                scrollback: raw.defaults.scrollback,
                master_ratio: MasterRatio(raw.defaults.master_ratio),
            },
        );
    }
    Ok(Config { workspaces })
}

fn validate_command(command: &[String], field: &str, source: &Path) -> Result<(), ConfigError> {
    if command.is_empty() || command.first().is_some_and(String::is_empty) {
        return Err(ConfigError::new(format!(
            "{}: {field} must be a non-empty argv array",
            source.display()
        )));
    }
    if command.iter().any(String::is_empty) {
        return Err(ConfigError::new(format!(
            "{}: {field} must not contain empty arguments",
            source.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::load;

    fn test_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("termdeck-config-{}-{nanos}", std::process::id()))
    }

    fn write_config(root: &Path, terminals: &str) -> PathBuf {
        let config = root.join("termdeck.yaml");
        fs::write(
            &config,
            format!(
                "version: 1\ndefaults:\n  command: [sh]\nworkspaces:\n  test:\n    root: {}\n    terminals:\n{terminals}",
                root.display()
            ),
        )
        .unwrap();
        config
    }

    #[test]
    fn resolves_paths_and_omits_missing_optional_projects() {
        let root = test_root();
        fs::create_dir_all(root.join("frontend")).unwrap();
        let config = write_config(
            &root,
            "      - name: frontend\n        cwd: frontend\n      - name: app\n        cwd: app\n        optional: true\n",
        );

        let loaded = load(config).unwrap();
        let workspace = &loaded.workspaces["test"];
        assert_eq!(workspace.projects.len(), 1);
        assert_eq!(workspace.projects[0].path, root.join("frontend"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn identifies_missing_required_project_paths() {
        let root = test_root();
        fs::create_dir(&root).unwrap();
        let config = write_config(&root, "      - name: frontend\n        cwd: frontend\n");

        let error = load(config).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("terminal 'frontend' path does not exist")
        );
        fs::remove_dir_all(root).unwrap();
    }
}
