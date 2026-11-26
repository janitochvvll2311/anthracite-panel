fn main() {
    if let Err(error) = cargo_anth::run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
