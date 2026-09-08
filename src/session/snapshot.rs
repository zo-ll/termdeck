//! Durable, daemon-less session snapshots.  They preserve layout and visible
//! text only; restoring always starts fresh child processes.

use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    cli,
    config::Workspace,
    contracts::{Project, TerminalEngine, TerminalId},
    ui::DeckState,
};

/// The snapshot format this engine writes and the picker reads.
pub const SCHEMA: &str = "session.v1";
pub const MAX_PANE_LINES: usize = 2_000;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Snapshot {
    pub schema: String,
    pub workspace: String,
    pub root: PathBuf,
    pub saved_at: u64,
    pub deck: SnapshotDeck,
    pub panes: Vec<SnapshotPane>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SnapshotDeck {
    pub order: Vec<usize>,
    pub zoomed: bool,
    pub collapsed: Vec<bool>,
    pub pinned: Option<usize>,
    pub master_ratio: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SnapshotPane {
    pub id: String,
    pub cwd: PathBuf,
    pub command: Vec<String>,
    pub shell_hook: bool,
    pub alt_screen: bool,
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotHeader {
    pub name: String,
    pub root: PathBuf,
    pub saved_at: u64,
    pub panes: usize,
}

#[derive(Deserialize)]
struct SnapshotHeaderWire {
    schema: String,
    workspace: String,
    root: PathBuf,
    saved_at: u64,
    panes: Vec<serde::de::IgnoredAny>,
}

#[derive(Clone, Debug)]
pub struct RestorePlan {
    pub snapshot: Snapshot,
    pub workspace: Workspace,
    pub deck: DeckState,
    pub skipped: Vec<String>,
}

pub fn sessions_dir() -> Result<PathBuf, String> {
    if let Some(state) = env::var_os("XDG_STATE_HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(state).join("termdeck/sessions"));
    }
    env::var_os("HOME")
        .filter(|path| !path.is_empty())
        .map(|home| PathBuf::from(home).join(".local/share/termdeck/sessions"))
        .ok_or_else(|| "cannot locate sessions: set XDG_STATE_HOME or HOME".to_owned())
}

pub fn list() -> Result<Vec<SnapshotHeader>, String> {
    let directory = sessions_dir()?;
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("cannot read {}: {error}", directory.display())),
    };
    let mut snapshots = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(snapshot) = serde_json::from_str::<SnapshotHeaderWire>(&source) else {
            continue;
        };
        if snapshot.schema == SCHEMA && validate_name(&snapshot.workspace).is_ok() {
            snapshots.push(SnapshotHeader {
                name: snapshot.workspace,
                root: snapshot.root,
                saved_at: snapshot.saved_at,
                panes: snapshot.panes.len(),
            });
        }
    }
    snapshots.sort_by_key(|item| std::cmp::Reverse(item.saved_at));
    Ok(snapshots)
}

pub fn save<E: TerminalEngine>(
    name: &str,
    workspace: &Workspace,
    projects: &[Project],
    deck: &DeckState,
    engine: &E,
) -> Result<SnapshotHeader, String> {
    validate_name(name)?;
    let (order, zoomed, collapsed, pinned, master_ratio) = deck.saved_layout();
    let panes = projects
        .iter()
        .map(|project| {
            let alt_screen = engine
                .metadata(&project.terminal)
                .is_some_and(|metadata| metadata.alt_screen);
            let capture_limit = MAX_PANE_LINES.saturating_sub(usize::from(alt_screen));
            let mut lines = if alt_screen {
                engine
                    .active_screen_lines(&project.terminal, capture_limit)
                    .unwrap_or_default()
            } else {
                engine
                    .history_lines(&project.terminal, capture_limit)
                    .unwrap_or_default()
            };
            if alt_screen {
                lines.insert(0, "(was running a full-screen app)".to_owned());
            }
            SnapshotPane {
                id: project.terminal.to_string(),
                cwd: project.path.clone(),
                command: project.command.clone(),
                shell_hook: project.shell_hook,
                alt_screen,
                lines,
            }
        })
        .collect();
    let snapshot = Snapshot {
        schema: SCHEMA.to_owned(),
        workspace: name.to_owned(),
        root: workspace.root.clone(),
        saved_at: unix_millis(),
        deck: SnapshotDeck {
            order,
            zoomed,
            collapsed,
            pinned,
            master_ratio,
        },
        panes,
    };
    write(&snapshot)?;
    Ok(SnapshotHeader {
        name: snapshot.workspace,
        root: snapshot.root,
        saved_at: snapshot.saved_at,
        panes: snapshot.panes.len(),
    })
}

pub fn load(name: &str) -> Result<Snapshot, String> {
    validate_name(name)?;
    load_file(&sessions_dir()?.join(format!("{name}.json")))
}

/// The same, for a caller that already holds the file: the picker lists the
/// directory itself, so it resumes the snapshot it listed rather than one
/// re-derived from a name.
pub fn load_file(path: &Path) -> Result<Snapshot, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("cannot read snapshot '{}': {error}", path.display()))?;
    let snapshot = serde_json::from_str::<Snapshot>(&source)
        .map_err(|error| format!("snapshot '{}' is corrupt: {error}", path.display()))?;
    validate(&snapshot)?;
    Ok(snapshot)
}

pub fn restore_plan(snapshot: Snapshot) -> Result<RestorePlan, String> {
    validate(&snapshot)?;
    let mut projects = Vec::new();
    let mut skipped = Vec::new();
    for pane in &snapshot.panes {
        if !pane.cwd.is_dir() {
            skipped.push(format!(
                "skipped {}: missing {}",
                pane.id,
                pane.cwd.display()
            ));
            continue;
        }
        projects.push(Project {
            terminal: TerminalId::new(pane.id.clone()),
            path: pane.cwd.clone(),
            command: pane.command.clone(),
            shell_hook: pane.shell_hook,
        });
    }
    if projects.is_empty() {
        if snapshot.root.is_dir() {
            let workspace = cli::discover_workspace(snapshot.root.clone()).map_err(|error| {
                format!("cannot rediscover {}: {error}", snapshot.root.display())
            })?;
            skipped.push("snapshot panes were unavailable; rediscovered workspace".to_owned());
            let deck = DeckState::new(workspace.projects.len())
                .with_master_ratio(snapshot.deck.master_ratio);
            return Ok(RestorePlan {
                snapshot,
                workspace,
                deck,
                skipped,
            });
        }
        return Err("snapshot has no pane directories that still exist".to_owned());
    }
    // If panes disappeared, their position-specific layout can no longer be
    // valid. Start a fresh arrangement while preserving the saved split.
    let deck = if projects.len() == snapshot.panes.len() {
        DeckState::restored(
            projects.len(),
            snapshot.deck.order.clone(),
            snapshot.deck.zoomed,
            snapshot.deck.collapsed.clone(),
            snapshot.deck.pinned,
            snapshot.deck.master_ratio,
        )?
    } else {
        DeckState::new(projects.len()).with_master_ratio(snapshot.deck.master_ratio)
    };
    let mut workspace = Workspace::discovered(snapshot.root.clone(), projects);
    workspace.name = snapshot.workspace.clone();
    Ok(RestorePlan {
        snapshot,
        workspace,
        deck,
        skipped,
    })
}

pub fn age(saved_at: u64) -> String {
    let elapsed = unix_millis().saturating_sub(saved_at) / 1_000;
    match elapsed {
        0..=59 => "just now".to_owned(),
        60..=3_599 => format!("{}m ago", elapsed / 60),
        3_600..=86_399 => format!("{}h ago", elapsed / 3_600),
        _ => format!("{}d ago", elapsed / 86_400),
    }
}

fn write(snapshot: &Snapshot) -> Result<(), String> {
    let directory = sessions_dir()?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    set_mode(&directory, 0o700)?;
    let path = directory.join(format!("{}.json", snapshot.workspace));
    let temporary = directory.join(format!(
        ".{}.{}.tmp",
        snapshot.workspace,
        std::process::id()
    ));
    let encoded = serde_json::to_vec(snapshot).map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| format!("cannot create {}: {error}", temporary.display()))?;
    set_mode(&temporary, 0o600)?;
    file.write_all(&encoded)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, &path)
        .map_err(|error| format!("cannot publish {}: {error}", path.display()))?;
    Ok(())
}

fn validate(snapshot: &Snapshot) -> Result<(), String> {
    if snapshot.schema != SCHEMA {
        return Err(format!(
            "unsupported snapshot schema '{}'; expected {SCHEMA}",
            snapshot.schema
        ));
    }
    validate_name(&snapshot.workspace)?;
    if snapshot.panes.is_empty() {
        return Err("snapshot has no panes".to_owned());
    }
    if snapshot
        .panes
        .iter()
        .any(|pane| pane.id.is_empty() || pane.command.is_empty())
    {
        return Err("snapshot contains an invalid pane".to_owned());
    }
    if snapshot
        .panes
        .iter()
        .any(|pane| pane.lines.len() > MAX_PANE_LINES)
    {
        return Err("snapshot exceeds the per-pane line limit".to_owned());
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err("snapshot name must be a non-empty file name".to_owned());
    }
    Ok(())
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| format!("cannot set permissions on {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn set_mode(_: &Path, _: u32) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use crate::{
        config::Workspace,
        contracts::{Project, TerminalId},
        engine::FakeEngine,
        ui::DeckState,
    };

    use super::{MAX_PANE_LINES, Snapshot, SnapshotDeck, SnapshotPane, restore_plan, save};

    fn root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("termdeck-snapshot-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn round_trip_captures_layout_and_a_bounded_transcript() {
        let root = root();
        fs::create_dir_all(root.join("pane")).unwrap();
        let project = Project {
            terminal: TerminalId::new("pane"),
            path: root.join("pane"),
            command: vec!["sh".to_owned()],
            shell_hook: true,
        };
        let workspace = Workspace::discovered(root.clone(), vec![project.clone()]);
        let mut engine = FakeEngine::new([project.terminal.clone()]);
        engine.set_history_lines(
            &project.terminal,
            (0..2_100).map(|n| n.to_string()).collect(),
        );
        let name = format!("snapshot-test-{}", std::process::id());
        let header = save(&name, &workspace, &[project], &DeckState::new(1), &engine).unwrap();
        assert_eq!(header.panes, 1);
        let snapshot = super::load(&name).unwrap();
        assert_eq!(snapshot.schema, "session.v1");
        assert_eq!(snapshot.panes[0].lines.len(), MAX_PANE_LINES);
        assert_eq!(snapshot.panes[0].lines[0], "100");
        fs::remove_file(super::sessions_dir().unwrap().join(format!("{name}.json"))).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_panes_are_skipped_without_losing_the_restore() {
        let root = root();
        fs::create_dir_all(root.join("present")).unwrap();
        let snapshot = Snapshot {
            schema: "session.v1".to_owned(),
            workspace: "demo".to_owned(),
            root: root.clone(),
            saved_at: 0,
            deck: SnapshotDeck {
                order: vec![0, 1],
                zoomed: true,
                collapsed: vec![false, true],
                pinned: None,
                master_ratio: 0.7,
            },
            panes: vec![
                SnapshotPane {
                    id: "present".to_owned(),
                    cwd: root.join("present"),
                    command: vec!["sh".to_owned()],
                    shell_hook: true,
                    alt_screen: false,
                    lines: vec![],
                },
                SnapshotPane {
                    id: "gone".to_owned(),
                    cwd: root.join("gone"),
                    command: vec!["sh".to_owned()],
                    shell_hook: true,
                    alt_screen: false,
                    lines: vec![],
                },
            ],
        };
        let plan = restore_plan(snapshot).unwrap();
        assert_eq!(plan.workspace.projects.len(), 1);
        assert_eq!(plan.skipped.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
