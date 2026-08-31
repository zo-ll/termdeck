fn main() {
    match termdeck::cli::run(std::env::args().skip(1)) {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("termdeck: {error}");
            std::process::exit(2);
        }
    }
}
