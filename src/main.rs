fn main() {
    if let Err(error) = malu::run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
