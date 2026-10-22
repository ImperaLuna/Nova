use clap::Parser;

fn main() {
    if let Err(e) = novad::run(novad::Args::parse()) {
        // The log may not be open yet: stderr reaches the terminal or journald.
        eprintln!("novad: {e}");
        std::process::exit(1);
    }
}
