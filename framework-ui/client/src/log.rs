//! What the client says about the connection. novad logs each line the UI
//! prints and reads the level from the line's first word.

const SOURCE: &str = "novad link";

pub(crate) fn info(message: &str) {
    eprintln!("INFO {SOURCE}: {message}");
}

pub(crate) fn warn(message: &str) {
    eprintln!("WARN {SOURCE}: {message}");
}
