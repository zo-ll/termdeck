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
    let config = termdeck::config::load(&intent.config_path)?;
    match &intent.command {
        termdeck::cli::CliCommand::Launch { workspace } => {
            let workspace = termdeck::cli::select_workspace(
                &config,
                &intent.config_path,
                workspace.as_deref(),
            )?;
            termdeck::session::run(&workspace)?;
            Ok(None)
        }
        termdeck::cli::CliCommand::Check | termdeck::cli::CliCommand::List => {
            termdeck::cli::inspect(&intent, &config).map_err(Into::into)
        }
    }
}
