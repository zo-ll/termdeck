//! Command-line parsing and configuration inspection.

use std::{env, error::Error, fmt, path::PathBuf};

use crate::config::load;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliEnvironment {
    pub xdg_config_home: Option<PathBuf>,
    pub home: Option<PathBuf>,
}

impl CliEnvironment {
    pub fn from_environment() -> Self {
        Self {
            xdg_config_home: env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            home: env::var_os("HOME").map(PathBuf::from),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CliCommand {
    Launch { workspace: Option<String> },
    Check,
    List,
}

/// Fully-owned CLI intent. Launch remains an intent until the composition
/// layer provides the UI; this module never creates a placeholder interface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliIntent {
    pub config_path: PathBuf,
    pub command: CliCommand,
}

#[derive(Debug)]
pub struct CliError(String);

impl CliError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Error for CliError {}

pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<CliIntent, CliError> {
    parse_with_environment(arguments, CliEnvironment::from_environment())
}

pub fn parse_with_environment(
    arguments: impl IntoIterator<Item = String>,
    environment: CliEnvironment,
) -> Result<CliIntent, CliError> {
    let mut arguments = arguments.into_iter();
    let mut config_path = None;
    let mut remaining = Vec::new();
    while let Some(argument) = arguments.next() {
        if argument == "--config" {
            if config_path.is_some() {
                return Err(CliError::new("--config can only be specified once"));
            }
            config_path = Some(PathBuf::from(
                arguments
                    .next()
                    .ok_or_else(|| CliError::new("--config requires a path"))?,
            ));
        } else if argument.starts_with('-') {
            return Err(CliError::new(format!("unknown option: {argument}")));
        } else {
            remaining.push(argument);
        }
    }

    let config_path = match config_path {
        Some(path) => path,
        None => default_config_path(&environment)?,
    };
    let command = match remaining.as_slice() {
        [] => CliCommand::Launch { workspace: None },
        [command] if command == "check" => CliCommand::Check,
        [command] if command == "list" => CliCommand::List,
        [workspace] => CliCommand::Launch {
            workspace: Some(workspace.clone()),
        },
        _ => return Err(CliError::new(usage())),
    };
    Ok(CliIntent {
        config_path,
        command,
    })
}

fn default_config_path(environment: &CliEnvironment) -> Result<PathBuf, CliError> {
    if let Some(path) = environment
        .xdg_config_home
        .as_deref()
        .filter(|path| !path.as_os_str().is_empty())
    {
        return Ok(path.join("termdeck/config.yaml"));
    }
    environment
        .home
        .as_deref()
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| path.join(".config/termdeck/config.yaml"))
        .ok_or_else(|| CliError::new("cannot locate config: set XDG_CONFIG_HOME or HOME"))
}

const fn usage() -> &'static str {
    "usage: termdeck [--config PATH] [WORKSPACE|check|list]"
}

pub fn run(arguments: impl IntoIterator<Item = String>) -> Result<Option<String>, Box<dyn Error>> {
    let intent = parse(arguments)?;
    let config = load(&intent.config_path)?;
    match intent.command {
        CliCommand::Launch { workspace } => {
            if let Some(workspace) = workspace
                && !config.workspaces.contains_key(&workspace)
            {
                return Err(CliError::new(format!(
                    "{}: workspace '{workspace}' is not configured",
                    intent.config_path.display()
                ))
                .into());
            }
            Ok(None)
        }
        CliCommand::Check => Ok(Some(format!(
            "{}: configuration valid ({} workspace{})",
            intent.config_path.display(),
            config.workspaces.len(),
            if config.workspaces.len() == 1 {
                ""
            } else {
                "s"
            }
        ))),
        CliCommand::List => Ok(Some(
            config
                .workspaces
                .values()
                .flat_map(|workspace| {
                    workspace.projects.iter().map(move |project| {
                        format!(
                            "{}\t{}\t{}\t{}",
                            workspace.name,
                            project.terminal,
                            project.path.display(),
                            project.command.join(" ")
                        )
                    })
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{CliCommand, CliEnvironment, parse_with_environment, run};

    fn environment() -> CliEnvironment {
        CliEnvironment {
            xdg_config_home: Some(PathBuf::from("/xdg")),
            home: Some(PathBuf::from("/home/test")),
        }
    }

    fn test_config() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("termdeck-cli-{}-{nanos}", std::process::id()));
        fs::create_dir_all(root.join("available")).unwrap();
        let config = root.join("termdeck.yaml");
        fs::write(
            &config,
            format!(
                "version: 1\ndefaults:\n  command: [sh]\nworkspaces:\n  test:\n    root: {}\n    terminals:\n      - name: available\n        cwd: available\n      - name: optional\n        cwd: optional\n        optional: true\n",
                root.display()
            ),
        )
        .unwrap();
        config
    }

    fn arguments(config: &Path, command: &str) -> [String; 3] {
        [
            "--config".to_owned(),
            config.display().to_string(),
            command.to_owned(),
        ]
    }

    #[test]
    fn list_and_check_are_commands() {
        assert_eq!(
            parse_with_environment(["list".to_owned()], environment())
                .unwrap()
                .command,
            CliCommand::List
        );
        assert_eq!(
            parse_with_environment(["check".to_owned()], environment())
                .unwrap()
                .command,
            CliCommand::Check
        );
    }

    #[test]
    fn explicit_config_overrides_default_for_commands() {
        let intent = parse_with_environment(
            ["--config", "chosen.yaml", "check"].map(str::to_owned),
            environment(),
        )
        .unwrap();

        assert_eq!(intent.config_path, PathBuf::from("chosen.yaml"));
        assert_eq!(intent.command, CliCommand::Check);
    }

    #[test]
    fn xdg_path_wins_over_home() {
        let intent = parse_with_environment(std::iter::empty(), environment()).unwrap();

        assert_eq!(
            intent.config_path,
            PathBuf::from("/xdg/termdeck/config.yaml")
        );
    }

    #[test]
    fn home_path_is_used_without_xdg() {
        let intent = parse_with_environment(
            std::iter::empty(),
            CliEnvironment {
                xdg_config_home: None,
                home: Some(PathBuf::from("/home/test")),
            },
        )
        .unwrap();

        assert_eq!(
            intent.config_path,
            PathBuf::from("/home/test/.config/termdeck/config.yaml")
        );
    }

    #[test]
    fn workspace_selection_is_a_launch_intent() {
        let intent = parse_with_environment(["idp".to_owned()], environment()).unwrap();

        assert_eq!(
            intent.command,
            CliCommand::Launch {
                workspace: Some("idp".to_owned()),
            }
        );
    }

    #[test]
    fn check_reports_a_valid_configuration() {
        let config = test_config();

        let output = run(arguments(&config, "check")).unwrap();

        assert!(output.unwrap().contains("configuration valid"));
        fs::remove_dir_all(config.parent().unwrap()).unwrap();
    }

    #[test]
    fn list_omits_missing_optional_projects() {
        let config = test_config();

        let output = run(arguments(&config, "list")).unwrap().unwrap();

        assert!(output.contains("test\tavailable\t"));
        assert!(!output.contains("\toptional\t"));
        fs::remove_dir_all(config.parent().unwrap()).unwrap();
    }

    #[test]
    fn unknown_workspace_is_actionable() {
        let config = test_config();

        let error = run(arguments(&config, "unknown")).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("workspace 'unknown' is not configured")
        );
        fs::remove_dir_all(config.parent().unwrap()).unwrap();
    }
}
