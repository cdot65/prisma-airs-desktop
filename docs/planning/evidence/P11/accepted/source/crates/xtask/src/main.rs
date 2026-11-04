fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = xtask::cli::main(args) {
        eprintln!("xtask: {error}");
        std::process::exit(1);
    }
}
