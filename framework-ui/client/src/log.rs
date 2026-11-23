const SOURCE: &str = "novad link";

pub(crate) fn info(message: &str) {
    eprintln!("INFO {SOURCE}: {message}");
}

pub(crate) fn warn(message: &str) {
    eprintln!("WARN {SOURCE}: {message}");
}
