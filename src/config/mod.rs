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
const DEFAULT_SCROLLBACK: usize = 10_000;

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

impl Workspace {
    /// Builds the same runtime shape as configuration loading for a workspace
    /// discovered from a folder.
    pub fn discovered(root: PathBuf, projects: Vec<Project>) -> Self {
        let name = root
            .file_name()
            .filter(|name| !name.is_empty())
            .unwrap_or(root.as_os_str())
            .to_string_lossy()
            .into_owned();
        Self {
            name,
            root,
            projects,
            scrollback: default_scrollback(),
            master_ratio: MasterRatio(default_master_ratio()),
        }
    }
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
    DEFAULT_SCROLLBACK
}

/// The top of the accepted range: a workspace that says nothing about the
/// split starts with the stack at its minimum width (#44). The interface
/// holds the same value; `tests::the_interfaces_split_range_is_the_one_this_file_validates`
/// keeps the two equal.
const fn default_master_ratio() -> f64 {
    0.85
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
        if workspace.terminals.is_empty() {
            return Err(ConfigError::new(format!(
                "{}: workspace '{name}' must define at least one terminal",
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

    /// #44: a workspace that says nothing about the split gets the minimum
    /// stack width, and one that does say something gets what it says. The
    /// default is a default, not a floor.
    #[test]
    fn a_configured_split_overrides_the_minimum_width_default() {
        let root = test_root();
        fs::create_dir_all(root.join("one")).unwrap();
        let config = write_config(&root, "      - name: one\n        cwd: one\n");

        let silent = load(&config).unwrap();
        assert_eq!(
            silent.workspaces["test"].master_ratio.get(),
            super::default_master_ratio(),
            "the stack starts at its minimum width"
        );
        assert_eq!(silent.workspaces["test"].master_ratio.get(), 0.85);

        let stated = root.join("stated.yaml");
        fs::write(
            &stated,
            fs::read_to_string(&config)
                .unwrap()
                .replace("  command: [sh]", "  command: [sh]\n  master_ratio: 0.60"),
        )
        .unwrap();

        let config = load(&stated).unwrap();
        assert_eq!(config.workspaces["test"].master_ratio.get(), 0.60);
        // And that is the split the deck opens with, not the default it
        // replaced: the seeding path the session uses.
        assert_eq!(
            crate::ui::DeckState::new(1)
                .with_master_ratio(config.workspaces["test"].master_ratio.get())
                .master_ratio(),
            0.60
        );
        fs::remove_dir_all(&root).unwrap();
    }

    /// The interface offers the split as a live gesture (#41), within its own
    /// copy of this range — the architecture boundary keeps `src/config` out
    /// of `src/ui`, so the two constants are duplicated rather than shared.
    /// Nothing else notices if one side moves, so this does: a drag or a
    /// nudge must never reach a split this file would reject, and the default
    /// a deck starts at must be the default this file hands it.
    #[test]
    fn the_interfaces_split_range_is_the_one_this_file_validates() {
        assert_eq!(super::MIN_MASTER_RATIO, crate::ui::MIN_MASTER_RATIO);
        assert_eq!(super::MAX_MASTER_RATIO, crate::ui::MAX_MASTER_RATIO);
        assert_eq!(
            super::default_master_ratio(),
            crate::ui::DEFAULT_MASTER_RATIO
        );
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

    #[test]
    fn accepts_more_than_four_terminals() {
        let root = test_root();
        let terminals = (0..8)
            .map(|index| {
                let name = format!("terminal-{index}");
                fs::create_dir_all(root.join(&name)).unwrap();
                format!("      - name: {name}\n        cwd: {name}\n")
            })
            .collect::<String>();
        let config = write_config(&root, &terminals);

        let loaded = load(config).unwrap();

        assert_eq!(loaded.workspaces["test"].projects.len(), 8);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_an_empty_workspace() {
        let root = test_root();
        fs::create_dir(&root).unwrap();
        let config = write_config(&root, "      []\n");

        let error = load(config).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("workspace 'test' must define at least one terminal")
        );
        fs::remove_dir_all(root).unwrap();
    }
}
