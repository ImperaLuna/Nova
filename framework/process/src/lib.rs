//! The programs novad runs ([`child`]), the facts it reads about
//! processes from `/proc` ([`procfs`]) and telling systemd it is up
//! ([`notify`]).

pub mod child;
pub mod notify;
pub mod procfs;
