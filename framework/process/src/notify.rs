use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::{SocketAddr, UnixDatagram};

pub const SOCKET_VAR: &str = "NOTIFY_SOCKET";
const READY: &[u8] = b"READY=1";
const SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

pub fn ready(socket: Option<&OsStr>) -> std::io::Result<bool> {
    let Some(socket) = socket.filter(|socket| !socket.is_empty()) else { return Ok(false) };
    let address = match socket.as_bytes().strip_prefix(b"@") {
        Some(name) => {
            use std::os::linux::net::SocketAddrExt;
            SocketAddr::from_abstract_name(name)?
        }
        None => SocketAddr::from_pathname(socket)?,
    };
    let socket = UnixDatagram::unbound()?;
    socket.set_write_timeout(Some(SEND_TIMEOUT))?;
    socket.send_to_addr(READY, &address)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn received(listener: &UnixDatagram) -> Vec<u8> {
        let mut buffer = [0; 64];
        let read = listener.recv(&mut buffer).unwrap();
        buffer[..read].to_vec()
    }

    #[test]
    fn ready_reaches_a_path_socket() {
        let dir = nova_test_support::TempDir::new("notify");
        let path = dir.join("notify");
        let _ = std::fs::remove_file(&path);
        let listener = UnixDatagram::bind(&path).unwrap();
        assert!(ready(Some(path.as_os_str())).unwrap());
        assert_eq!(received(&listener), READY);
    }

    #[test]
    fn ready_reaches_an_abstract_socket() {
        use std::os::linux::net::SocketAddrExt;
        let name = format!("novad-notify-test-{}", std::process::id());
        let listener = UnixDatagram::bind_addr(&SocketAddr::from_abstract_name(name.as_bytes()).unwrap()).unwrap();
        assert!(ready(Some(OsStr::new(&format!("@{name}")))).unwrap());
        assert_eq!(received(&listener), READY);
    }

    #[test]
    fn nothing_to_tell_without_a_socket() {
        assert!(!ready(None).unwrap());
        assert!(!ready(Some(OsStr::new(""))).unwrap());
    }
}
