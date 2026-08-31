fn main() {
    match termdeck::cli::run(std::env::args().skip(1)) {
        Ok(Some(output)) => println!("{output}"),
        Ok(None) => {}
        Err(error) => {
            eprintln!("termdeck: {error}");
            std::process::exit(2);
        }
    }
}
