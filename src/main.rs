fn main() {
    if let Err(error) = skillsupport::cli::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
