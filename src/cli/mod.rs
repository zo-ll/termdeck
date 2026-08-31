//! Minimal command-line entry points for inspecting configuration.

use std::{error::Error, path::Path};

use crate::config::load;

pub fn run(arguments: impl IntoIterator<Item = String>) -> Result<String, Box<dyn Error>> {
    let arguments: Vec<_> = arguments.into_iter().collect();
    let (command, path) = match arguments.as_slice() {
        [command, path] if command == "check" || command == "list" => (command.as_str(), path),
        _ => return Err("usage: termdeck <check|list> <CONFIG>".into()),
    };
    let config = load(Path::new(path))?;
    match command {
        "check" => Ok(format!(
            "{}: configuration valid ({} workspace{})",
            path,
            config.workspaces.len(),
            if config.workspaces.len() == 1 {
                ""
            } else {
                "s"
            }
        )),
        "list" => Ok(config
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
            .join("\n")),
        _ => unreachable!("matched above"),
    }
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn rejects_incomplete_commands() {
        assert!(run(["check".to_owned()]).is_err());
    }
}
