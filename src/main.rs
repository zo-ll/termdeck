fn main() {
    match run() {
        Ok(Some(output)) => println!("{output}"),
        Ok(None) => {}
        Err(error) => {
            eprintln!("termdeck: {error}");
            let exit_code = error
                .downcast_ref::<termdeck::cli::CliError>()
                .map_or(2, termdeck::cli::CliError::exit_code);
            if exit_code == 3 {
                eprintln!(
                    "\n{}\nTry `termdeck --help` for more information.",
                    termdeck::cli::usage()
                );
            }
            std::process::exit(i32::from(exit_code));
        }
    }
}

fn run() -> Result<Option<String>, Box<dyn std::error::Error>> {
    run_with_arguments(std::env::args().skip(1))
}

fn run_with_arguments(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let intent = termdeck::cli::parse(arguments)?;
    match &intent.command {
        termdeck::cli::CliCommand::Folder { root } => {
            let workspace = termdeck::cli::discover_workspace(root)?;
            termdeck::session::run(&workspace)?;
            Ok(None)
        }
        termdeck::cli::CliCommand::Picker => {
            // No path: the picker chooses the workspace, then the session
            // opens it. Cancelling is successful, but says what happened
            // after the alternate screen has been restored.
            match termdeck::session::pick(termdeck::cli::picker_roots())? {
                Some(workspace) => {
                    termdeck::session::run(&workspace)?;
                    Ok(None)
                }
                None => Ok(Some("picker cancelled".to_owned())),
            }
        }
        termdeck::cli::CliCommand::Help(topic) => Ok(Some(termdeck::cli::help(*topic).to_owned())),
        termdeck::cli::CliCommand::Launch { workspace } => {
            let config_path = intent.required_config_path()?;
            let config = termdeck::config::load(config_path)?;
            let workspace =
                termdeck::cli::select_workspace(&config, config_path, workspace.as_deref())?;
            termdeck::session::run(&workspace)?;
            Ok(None)
        }
        termdeck::cli::CliCommand::Check | termdeck::cli::CliCommand::List => {
            let config_path = intent.required_config_path()?;
            let config = termdeck::config::load(config_path)?;
            termdeck::cli::inspect(&intent, &config).map_err(Into::into)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::run_with_arguments;

    #[test]
    fn general_and_command_help_do_not_need_a_configuration() {
        for arguments in [
            vec!["--help"],
            vec!["help"],
            vec!["check", "--help"],
            vec!["help", "list"],
        ] {
            let output = run_with_arguments(arguments.into_iter().map(str::to_owned))
                .unwrap()
                .unwrap();

            assert!(output.contains("USAGE"), "{output}");
            assert!(output.contains("TERMDECK"), "{output}");
        }
    }
}
