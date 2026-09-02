fn main() {
    match run() {
        Ok(Some(output)) => println!("{output}"),
        Ok(None) => {}
        Err(error) => {
            eprintln!("termdeck: {error}");
            std::process::exit(2);
        }
    }
}

fn run() -> Result<Option<String>, Box<dyn std::error::Error>> {
    let intent = termdeck::cli::parse(std::env::args().skip(1))?;
    match &intent.command {
        termdeck::cli::CliCommand::Folder { root } => {
            let workspace = termdeck::cli::discover_workspace(root)?;
            termdeck::session::run(&workspace)?;
            Ok(None)
        }
        termdeck::cli::CliCommand::Picker => Err("folder picker pending A2".into()),
        termdeck::cli::CliCommand::Launch { workspace } => {
            let config_path = intent.config_path.as_deref().expect("config command");
            let config = termdeck::config::load(config_path)?;
            let workspace =
                termdeck::cli::select_workspace(&config, config_path, workspace.as_deref())?;
            termdeck::session::run(&workspace)?;
            Ok(None)
        }
        termdeck::cli::CliCommand::Check | termdeck::cli::CliCommand::List => {
            let config_path = intent.config_path.as_deref().expect("config command");
            let config = termdeck::config::load(config_path)?;
            termdeck::cli::inspect(&intent, &config).map_err(Into::into)
        }
    }
}
