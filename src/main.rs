fn main() {
    if let Err(error) = maludb::run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
