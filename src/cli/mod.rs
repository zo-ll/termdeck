//! Command-line parsing and configuration inspection.

use std::{
    env,
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
};

use crate::{
    config::{Config, Workspace, load},
    contracts::{Project, TerminalId},
};

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
    Folder { root: PathBuf },
    Picker,
    Check,
    List,
    Help(Option<CliHelp>),
}

/// The command-specific page requested through `termdeck help COMMAND`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CliHelp {
    Check,
    List,
}

/// Fully-owned CLI intent. Launch remains an intent until the composition
/// layer provides the UI; this module never creates a placeholder interface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliIntent {
    pub config_path: Option<PathBuf>,
    pub command: CliCommand,
}

impl CliIntent {
    /// Commands that inspect configured workspaces require this path. Parsing
    /// normally supplies it; keep malformed programmatic intents recoverable.
    pub fn required_config_path(&self) -> Result<&Path, CliError> {
        self.config_path
            .as_deref()
            .ok_or_else(|| CliError::new("configuration command has no config path"))
    }
}

#[derive(Debug)]
pub struct CliError {
    message: String,
    exit_code: u8,
}

impl CliError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: 2,
        }
    }

    fn usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: 3,
        }
    }

    /// Exit status for errors reported directly by the `termdeck` binary.
    pub const fn exit_code(&self) -> u8 {
        self.exit_code
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
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
    // Whether `--` has ended the verbs (#133). `check` and `list` are words a
    // folder or a workspace is allowed to be called, and a bare one of those
    // is the command: nothing else could be, since the meaning of a command
    // must not depend on what happens to sit in the working directory. So
    // there has to be a way to say "this token is a name", and `--` is the
    // one every shell user already knows.
    let mut literal = false;
    let mut help = false;
    while let Some(argument) = arguments.next() {
        if argument == "--" {
            // Everything after it is positional, a leading `-` included:
            // that is what ending option parsing means everywhere else.
            literal = true;
            remaining.extend(arguments.by_ref());
            break;
        }
        if argument == "--config" {
            if config_path.is_some() {
                return Err(CliError::usage("--config can only be specified once"));
            }
            config_path =
                Some(PathBuf::from(arguments.next().ok_or_else(|| {
                    CliError::usage("--config requires a path")
                })?));
        } else if matches!(argument.as_str(), "--help" | "-h") {
            help = true;
        } else if argument.starts_with('-') {
            return Err(CliError::usage(format!("unknown option: {argument}")));
        } else {
            remaining.push(argument);
        }
    }

    let help_topic = |topic: &str| match topic {
        "check" => Ok(CliHelp::Check),
        "list" => Ok(CliHelp::List),
        _ => Err(CliError::usage(format!("unknown command: {topic}"))),
    };
    if help {
        let topic = match remaining.as_slice() {
            [] => None,
            [topic] => Some(help_topic(topic)?),
            _ => return Err(CliError::usage("help accepts at most one command")),
        };
        return Ok(CliIntent {
            config_path: None,
            command: CliCommand::Help(topic),
        });
    }

    let command = match remaining.as_slice() {
        [] if config_path.is_some() => CliCommand::Launch { workspace: None },
        [] => CliCommand::Picker,
        [command] if !literal && command == "help" => CliCommand::Help(None),
        [command, topic] if !literal && command == "help" => {
            CliCommand::Help(Some(help_topic(topic)?))
        }
        [command] if !literal && command == "check" => CliCommand::Check,
        [command] if !literal && command == "list" => CliCommand::List,
        [workspace] if config_path.is_some() => CliCommand::Launch {
            workspace: Some(workspace.clone()),
        },
        [path] if config_path.is_none() => resolve_path(path, &mut config_path)?,
        _ => return Err(CliError::usage("too many arguments")),
    };
    let config_path = match &command {
        CliCommand::Launch { .. } | CliCommand::Check | CliCommand::List => {
            Some(match config_path {
                Some(path) => path,
                None => default_config_path(&environment)?,
            })
        }
        CliCommand::Folder { .. } | CliCommand::Picker | CliCommand::Help(_) => None,
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

pub const fn usage() -> &'static str {
    // Termdeck has no protocol-level refusal result. It reserves 3 for a
    // malformed invocation, while a resolved path/configuration failure stays
    // 2; termctl instead reserves 3 for a well-formed request the session
    // declines and therefore keeps parser failures at 2.
    "usage: termdeck [OPTIONS] [FOLDER|CONFIG_FILE|COMMAND]"
}

/// Man-page-shaped command help. It describes the present positional resolver
/// rather than the older named-workspace invocation documented elsewhere.
pub const fn help(topic: Option<CliHelp>) -> &'static str {
    match topic {
        Some(CliHelp::Check) => {
            "TERMDECK-CHECK\n\nUSAGE\n  termdeck [--config PATH] check\n\nValidates the selected configuration without opening a terminal session.\n\nOPTIONS\n  --config PATH  Read this configuration file.\n  -h, --help     Show this help page.\n\nRun `termdeck --help` for environment variables and examples."
        }
        Some(CliHelp::List) => {
            "TERMDECK-LIST\n\nUSAGE\n  termdeck [--config PATH] list\n\nLists resolved terminals in the selected configuration.\n\nOPTIONS\n  --config PATH  Read this configuration file.\n  -h, --help     Show this help page.\n\nRun `termdeck --help` for environment variables and examples."
        }
        None => {
            "TERMDECK\n\nUSAGE\n  termdeck [OPTIONS] [FOLDER|CONFIG_FILE|COMMAND]\n  termdeck --config PATH [WORKSPACE]\n  termdeck -- NAME\n\nOpen a terminal workspace. With no positional argument, opens the folder picker. A directory opens a discovered workspace; a configuration file opens its configured workspace.\n\nCOMMANDS\n  check          Validate configuration without opening a session.\n  list           List resolved configured terminals.\n  help [COMMAND] Show general or command-specific help.\n\nOPTIONS\n  --config PATH  Use PATH instead of the default configuration file.\n  -- NAME        End option and command parsing; treat NAME as a path or workspace.\n  -h, --help     Show this help page.\n\nENVIRONMENT\n  XDG_CONFIG_HOME, HOME       Locate the default configuration file.\n  TERMDECK_SOCK               Session socket provided to child panes.\n  TERMDECK_PANE               Current pane identity provided to child panes.\n  TERMDECK_NOTIFY             Enable automatic shell completion notifications.\n  TERMDECK_NOTIFY_LONG_SECS   Long-command threshold in seconds.\n  TERMDECK_ALLOW_INPUT        Permit termctl input requests from this pane.\n\nEXAMPLES\n  termdeck\n  termdeck ./project\n  termdeck ./termdeck.yaml\n  termdeck --config ./termdeck.yaml workspace\n  termdeck --config ./termdeck.yaml list\n  termdeck -- check\n  termdeck help check"
        }
    }
}

pub fn run(arguments: impl IntoIterator<Item = String>) -> Result<Option<String>, Box<dyn Error>> {
    let intent = parse(arguments)?;
    match &intent.command {
        CliCommand::Folder { .. } => Ok(None),
        // The binary owns the interactive picker, so this helper does not
        // attempt to open one.
        CliCommand::Picker => Ok(None),
        CliCommand::Help(topic) => Ok(Some(help(*topic).to_owned())),
        CliCommand::Launch { .. } | CliCommand::Check | CliCommand::List => {
            let config_path = intent.required_config_path()?;
            let config = load(config_path)?;
            Ok(inspect(&intent, &config)?)
        }
    }
}

/// Resolves the one positional entry without guessing whether a misspelled
/// folder was meant to be a workspace name.
fn resolve_path(path: &str, config_path: &mut Option<PathBuf>) -> Result<CliCommand, CliError> {
    let metadata = fs::metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            CliError::new(format!("{path}: path does not exist"))
        } else {
            CliError::new(format!("cannot access {path}: {error}"))
        }
    })?;
    if metadata.is_file() {
        *config_path = Some(PathBuf::from(path));
        Ok(CliCommand::Launch { workspace: None })
    } else if metadata.is_dir() {
        Ok(CliCommand::Folder {
            root: PathBuf::from(path),
        })
    } else {
        Err(CliError::new(format!("{path}: not a file or directory")))
    }
}

/// The roots the picker browses when `termdeck` is given no path.
///
/// The picker's design note leaves the schema to configuration and consumes a
/// resolved list; until a `roots:` key exists, that list is the working
/// directory and the user's home, which is where a folder browser opened with
/// no argument is expected to start.
pub fn picker_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        roots.push(cwd);
    }
    if let Some(home) = CliEnvironment::from_environment().home
        && !roots.contains(&home)
    {
        roots.push(home);
    }
    roots
}

/// Finds direct repositories and repositories in the conventional component
/// folders. The returned projects can be passed directly to `Workspace`.
pub fn discover(root: &Path) -> Result<Vec<Project>, CliError> {
    let mut projects = Vec::new();
    for (folder, prefix) in [("frontends", "fe"), ("backends", "be"), ("apps", "app")] {
        let group = root.join(folder);
        if group.is_dir() {
            projects.extend(repositories(&group, prefix)?);
        }
    }
    projects.extend(repositories(root, "")?);
    Ok(projects)
}

/// Resolves a folder launch to the frozen runtime workspace shape. A folder
/// without discovered child repositories is always one terminal of its own.
pub fn discover_workspace(root: impl Into<PathBuf>) -> Result<Workspace, CliError> {
    let root = root.into();
    if !root.is_dir() {
        return Err(CliError::new(format!(
            "{}: not a directory",
            root.display()
        )));
    }
    let projects = if is_repository(&root) {
        Vec::new()
    } else {
        discover(&root)?
    };
    let projects = if projects.is_empty() {
        vec![project(&root, "")]
    } else {
        projects
    };
    Ok(Workspace::discovered(root, projects))
}

fn repositories(root: &Path, prefix: &str) -> Result<Vec<Project>, CliError> {
    let mut entries = fs::read_dir(root)
        .map_err(|error| CliError::new(format!("cannot read {}: {error}", root.display())))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| CliError::new(format!("cannot read {}: {error}", root.display())))?;
    entries.sort_by_key(|entry| entry.file_name());
    Ok(entries
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && is_repository(path))
        .map(|path| project(&path, prefix))
        .collect())
}

fn is_repository(path: &Path) -> bool {
    path.join(".git").is_dir() || path.join(".git").is_file()
}

fn project(path: &Path, prefix: &str) -> Project {
    let name = path
        .file_name()
        .filter(|name| !name.is_empty())
        .unwrap_or(path.as_os_str())
        .to_string_lossy();
    let name = if prefix.is_empty() {
        name.into_owned()
    } else {
        format!("{prefix}-{name}")
    };
    Project {
        terminal: TerminalId::new(name),
        path: path.to_path_buf(),
        command: vec!["bash".to_owned(), "-l".to_owned()],
        shell_hook: true,
    }
}

/// Returns the configured workspace that an interactive session will open.
/// A single workspace is unambiguous; multiple workspaces require the existing
/// positional selector until the planned picker has a dedicated UI surface.
pub fn select_workspace(
    config: &Config,
    config_path: &Path,
    requested: Option<&str>,
) -> Result<Workspace, CliError> {
    match requested {
        Some(name) => config.workspaces.get(name).cloned().ok_or_else(|| {
            CliError::new(format!(
                "{}: workspace '{name}' is not configured",
                config_path.display()
            ))
        }),
        None if config.workspaces.len() == 1 => config
            .workspaces
            .values()
            .next()
            .cloned()
            .ok_or_else(|| CliError::new("configuration has no workspace")),
        None => Err(CliError::new(format!(
            "{}: choose a workspace ({})",
            config_path.display(),
            config
                .workspaces
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// Formats the non-interactive commands and validates a launch selection.
pub fn inspect(intent: &CliIntent, config: &Config) -> Result<Option<String>, CliError> {
    let config_path = intent.required_config_path()?;
    match &intent.command {
        CliCommand::Launch { workspace } => {
            select_workspace(config, config_path, workspace.as_deref())?;
            Ok(None)
        }
        CliCommand::Check => Ok(Some(format!(
            "{}: configuration valid ({} workspace{})",
            config_path.display(),
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
        CliCommand::Folder { .. } | CliCommand::Picker | CliCommand::Help(_) => Err(CliError::new(
            "folder and picker commands do not inspect configuration",
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

    use super::{
        CliCommand, CliEnvironment, CliHelp, CliIntent, discover, discover_workspace, help,
        parse_with_environment, run,
    };

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

    fn test_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("termdeck-discover-{}-{nanos}", std::process::id()))
    }

    fn repository(path: &Path) {
        fs::create_dir_all(path.join(".git")).unwrap();
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

    /// #133: `check` and `list` are ordinary words, and a folder or a
    /// workspace is allowed to be called one. The bare token stays the
    /// command — a command whose meaning depended on what happened to be in
    /// the working directory would be worse than the shadowing — so `--`
    /// ends the verbs and whatever follows is a name.
    #[test]
    fn a_folder_named_like_a_command_is_reachable_after_a_double_dash() {
        let root = test_root();
        let shadowed = root.join("check");
        let help = root.join("help");
        fs::create_dir_all(&shadowed).unwrap();
        fs::create_dir_all(&help).unwrap();

        // The bare verb is still the verb, wherever it is run.
        assert_eq!(
            parse_with_environment(["check".to_owned()], environment())
                .unwrap()
                .command,
            CliCommand::Check
        );

        let intent = parse_with_environment(
            ["--".to_owned(), shadowed.display().to_string()],
            environment(),
        )
        .unwrap();
        assert_eq!(intent.command, CliCommand::Folder { root: shadowed });
        assert_eq!(intent.config_path, None, "a folder reads no configuration");

        let literal_help =
            parse_with_environment(["--".to_owned(), help.display().to_string()], environment())
                .unwrap();
        assert_eq!(literal_help.command, CliCommand::Folder { root: help });

        let literal_flag =
            parse_with_environment(["--".to_owned(), "--help".to_owned()], environment())
                .unwrap_err();
        assert_eq!(literal_flag.to_string(), "--help: path does not exist");
        assert_eq!(literal_flag.exit_code(), 2, "a literal path is not help");

        fs::remove_dir_all(&root).unwrap();
    }

    /// The half with no workaround at all: a path can already be spelled
    /// `./check`, but a workspace name is not a path and had nothing to
    /// disambiguate it — `--config file.yaml check` validated the file
    /// instead of opening the workspace called `check` (#133).
    #[test]
    fn a_workspace_named_like_a_verb_is_reachable_after_a_double_dash() {
        let config = test_config();

        let verb = parse_with_environment(
            ["--config", &config.display().to_string(), "check"].map(str::to_owned),
            environment(),
        )
        .unwrap();
        assert_eq!(verb.command, CliCommand::Check, "the verb still wins");

        let workspace = parse_with_environment(
            ["--config", &config.display().to_string(), "--", "check"].map(str::to_owned),
            environment(),
        )
        .unwrap();
        assert_eq!(
            workspace.command,
            CliCommand::Launch {
                workspace: Some("check".to_owned()),
            }
        );
        assert_eq!(workspace.config_path, Some(config.clone()));

        fs::remove_dir_all(config.parent().unwrap()).unwrap();
    }

    /// The escape hatch that already worked, pinned so it keeps working: a
    /// token that is spelled as a path was never a verb, because it is not
    /// the word. And `--` with nothing behind it changes nothing.
    #[test]
    fn a_path_spelling_and_a_bare_double_dash_keep_their_meanings() {
        let root = test_root();
        let shadowed = root.join("list");
        fs::create_dir_all(&shadowed).unwrap();
        let inside = std::env::current_dir().unwrap();
        assert!(shadowed.is_absolute(), "the test never depends on the cwd");

        let intent =
            parse_with_environment([shadowed.display().to_string()], environment()).unwrap();
        assert_eq!(
            intent.command,
            CliCommand::Folder {
                root: shadowed.clone()
            },
            "`termdeck <path>/list` is a path with or without the `--`"
        );

        assert_eq!(
            parse_with_environment(["--".to_owned()], environment())
                .unwrap()
                .command,
            CliCommand::Picker,
            "nothing to name is still the picker"
        );
        assert_eq!(
            std::env::current_dir().unwrap(),
            inside,
            "no cwd was harmed"
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn general_and_command_help_parse_without_loading_configuration() {
        let general = parse_with_environment(["--help".to_owned()], environment()).unwrap();
        assert_eq!(general.command, CliCommand::Help(None));
        assert_eq!(general.config_path, None);

        let check =
            parse_with_environment(["help", "check"].map(str::to_owned), environment()).unwrap();
        assert_eq!(check.command, CliCommand::Help(Some(CliHelp::Check)));

        let list =
            parse_with_environment(["list", "--help"].map(str::to_owned), environment()).unwrap();
        assert_eq!(list.command, CliCommand::Help(Some(CliHelp::List)));
        assert!(help(None).contains("TERMDECK_SOCK"));
        assert!(help(Some(CliHelp::Check)).contains("USAGE"));
    }

    #[test]
    fn explicit_config_overrides_default_for_commands() {
        let intent = parse_with_environment(
            ["--config", "chosen.yaml", "check"].map(str::to_owned),
            environment(),
        )
        .unwrap();

        assert_eq!(intent.config_path, Some(PathBuf::from("chosen.yaml")));
        assert_eq!(intent.command, CliCommand::Check);
    }

    #[test]
    fn a_programmatic_config_command_without_a_path_is_actionable() {
        let intent = CliIntent {
            config_path: None,
            command: CliCommand::Check,
        };

        assert_eq!(
            intent.required_config_path().unwrap_err().to_string(),
            "configuration command has no config path"
        );
    }

    #[test]
    fn xdg_path_wins_over_home() {
        let intent = parse_with_environment(["check".to_owned()], environment()).unwrap();

        assert_eq!(
            intent.config_path,
            Some(PathBuf::from("/xdg/termdeck/config.yaml"))
        );
    }

    #[test]
    fn home_path_is_used_without_xdg() {
        let intent = parse_with_environment(
            ["check".to_owned()],
            CliEnvironment {
                xdg_config_home: None,
                home: Some(PathBuf::from("/home/test")),
            },
        )
        .unwrap();

        assert_eq!(
            intent.config_path,
            Some(PathBuf::from("/home/test/.config/termdeck/config.yaml"))
        );
    }

    #[test]
    fn configured_workspace_selection_is_a_launch_intent() {
        let intent = parse_with_environment(
            ["--config", "termdeck.yaml", "idp"].map(str::to_owned),
            environment(),
        )
        .unwrap();

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

    #[test]
    fn a_folder_and_a_file_resolve_to_different_entry_points() {
        let root = test_root();
        fs::create_dir_all(&root).unwrap();
        let config = root.join("termdeck.yaml");
        fs::write(&config, "version: 1").unwrap();

        let folder = parse_with_environment([root.display().to_string()], environment()).unwrap();
        assert_eq!(folder.config_path, None);
        assert_eq!(folder.command, CliCommand::Folder { root: root.clone() });

        let file = parse_with_environment([config.display().to_string()], environment()).unwrap();
        assert_eq!(file.config_path, Some(config));
        assert_eq!(file.command, CliCommand::Launch { workspace: None });
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn no_path_is_the_picker_entry_point() {
        let intent = parse_with_environment(std::iter::empty(), environment()).unwrap();

        assert_eq!(intent.config_path, None);
        assert_eq!(intent.command, CliCommand::Picker);
    }

    #[test]
    fn a_missing_folder_names_the_problem() {
        let missing = test_root();

        let error = parse_with_environment([missing.display().to_string()], environment())
            .unwrap_err()
            .to_string();

        assert!(error.contains(&format!("{}: path does not exist", missing.display())));
    }

    #[test]
    fn an_unknown_option_names_the_option() {
        let error = parse_with_environment(["--unknown".to_owned()], environment()).unwrap_err();

        assert_eq!(error.to_string(), "unknown option: --unknown");
        assert_eq!(error.exit_code(), 3);
    }

    #[test]
    fn a_non_repository_folder_becomes_one_terminal() {
        let root = test_root();
        fs::create_dir_all(&root).unwrap();

        let workspace = discover_workspace(root.clone()).unwrap();

        assert_eq!(workspace.projects.len(), 1);
        assert_eq!(workspace.projects[0].path, root);
        assert_eq!(workspace.projects[0].command, ["bash", "-l"]);
        assert!(workspace.projects[0].shell_hook);
        fs::remove_dir_all(workspace.root).unwrap();
    }

    #[test]
    fn a_repository_folder_stays_one_terminal_even_with_child_repositories() {
        let root = test_root();
        repository(&root);
        repository(&root.join("child"));

        let workspace = discover_workspace(root.clone()).unwrap();

        assert_eq!(workspace.projects.len(), 1);
        assert_eq!(workspace.projects[0].path, root);
        fs::remove_dir_all(workspace.root).unwrap();
    }

    #[test]
    fn discovery_classifies_conventional_repositories() {
        let root = test_root();
        repository(&root.join("frontends/shop"));
        repository(&root.join("frontends/admin"));
        repository(&root.join("backends/api"));
        repository(&root.join("apps/mobile"));
        repository(&root.join("worker"));
        fs::create_dir_all(root.join("notes")).unwrap();

        let projects = discover(&root).unwrap();
        let names = projects
            .iter()
            .map(|project| project.terminal.to_string())
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            ["fe-admin", "fe-shop", "be-api", "app-mobile", "worker"]
        );
        assert!(
            projects
                .iter()
                .all(|project| project.command == ["bash", "-l"])
        );
        assert!(projects.iter().all(|project| project.shell_hook));
        fs::remove_dir_all(root).unwrap();
    }
}
