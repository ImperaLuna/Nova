use pulseaudio::protocol::{
    self, AuthParams, AuthReply, Command, CommandReply, Prop, Props, ProtocolError, ServerInfo, SetClientNameReply,
    SinkInfoList, SinkInputInfoList, SourceInfoList, SourceOutputInfoList, SubscriptionMask,
};
use std::ffi::CString;
use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);
const CLIENT_NAME: &str = "Nova";
pub(crate) const COOKIE_LENGTH: usize = 256;
const SOCKET_IN_RUNTIME_DIR: &str = "pulse/native";
const UNIX_PREFIX: &str = "unix:";

#[derive(Debug, Clone, Default)]
pub struct Sources {
    pub socket: Option<PathBuf>,
}

impl Sources {
    pub fn system() -> Self {
        let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        let socket = match std::env::var("PULSE_SERVER").ok().filter(|server| !server.is_empty()) {
            Some(server) => socket_named(&server),
            None => var("PULSE_RUNTIME_PATH")
                .map(|dir| dir.join("native"))
                .or_else(|| var("XDG_RUNTIME_DIR").map(|dir| dir.join(SOCKET_IN_RUNTIME_DIR))),
        };
        Self { socket }
    }
}

fn socket_named(servers: &str) -> Option<PathBuf> {
    servers.split_whitespace().find_map(|server| {
        let server = match server.strip_prefix('{') {
            Some(rest) => rest.split_once('}')?.1,
            None => server,
        };
        let path = server.strip_prefix(UNIX_PREFIX).unwrap_or(server);
        path.starts_with('/').then(|| PathBuf::from(path))
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    pub server: ServerInfo,
    pub sinks: SinkInfoList,
    pub sources: SourceInfoList,
    pub sink_inputs: SinkInputInfoList,
    pub source_outputs: SourceOutputInfoList,
}

#[derive(Debug)]
pub enum Failed {
    Refused(String),
    Broken(String),
}

pub struct Link {
    reader: BufReader<UnixStream>,
    version: u16,
    next_request: u32,
}

impl Link {
    // PulseAudio trusts its own socket's credentials and pipewire-pulse checks only the cookie's length, so the cookie is empty:
    // https://gitlab.freedesktop.org/pipewire/pipewire/-/tree/master/src/modules/module-protocol-pulse
    pub fn connect(sources: &Sources) -> Result<Self, String> {
        let socket = sources.socket.as_deref().ok_or("nothing says where the sound server's socket is")?;
        let stream = UnixStream::connect(socket).map_err(|e| format!("can't connect to {}: {e}", socket.display()))?;
        stream
            .set_read_timeout(Some(ANSWER_TIMEOUT))
            .and_then(|()| stream.set_write_timeout(Some(ANSWER_TIMEOUT)))
            .map_err(|e| format!("can't set the socket's timeouts: {e}"))?;
        let mut link = Self { reader: BufReader::new(stream), version: protocol::MAX_VERSION, next_request: 0 };
        let auth = AuthParams {
            version: protocol::MAX_VERSION,
            supports_shm: false,
            supports_memfd: false,
            cookie: vec![0; COOKIE_LENGTH],
        };
        let reply: AuthReply = link.ask(&Command::Auth(auth))?;
        link.version = reply.version.min(protocol::MAX_VERSION);
        let mut props = Props::new();
        props.set(Prop::ApplicationName, CString::new(CLIENT_NAME).map_err(|e| e.to_string())?);
        let _: SetClientNameReply = link.ask(&Command::SetClientName(props))?;
        Ok(link)
    }

    pub fn listing(&mut self) -> Result<Listing, String> {
        Ok(Listing {
            server: self.ask(&Command::GetServerInfo)?,
            sinks: self.ask(&Command::GetSinkInfoList)?,
            sources: self.ask(&Command::GetSourceInfoList)?,
            sink_inputs: self.ask(&Command::GetSinkInputInfoList)?,
            source_outputs: self.ask(&Command::GetSourceOutputInfoList)?,
        })
    }

    pub fn tell(&mut self, command: &Command) -> Result<(), Failed> {
        let request = self.send(command).map_err(Failed::Broken)?;
        match protocol::read_ack_message(&mut self.reader) {
            Ok(answered) => same_request(request, answered).map_err(Failed::Broken),
            Err(ProtocolError::ServerError(error)) => Err(Failed::Refused(format!("the sound server said: {error}"))),
            Err(e) => Err(Failed::Broken(e.to_string())),
        }
    }

    pub fn follow(&mut self) -> Result<(), String> {
        let mask = SubscriptionMask::SINK
            | SubscriptionMask::SOURCE
            | SubscriptionMask::SINK_INPUT
            | SubscriptionMask::SOURCE_OUTPUT
            | SubscriptionMask::SERVER;
        let request = self.send(&Command::Subscribe(mask))?;
        let answered = protocol::read_ack_message(&mut self.reader).map_err(|e| format!("subscribe: {e}"))?;
        same_request(request, answered)?;
        self.reader.get_ref().set_read_timeout(None).map_err(|e| format!("can't clear the socket's timeout: {e}"))
    }

    pub fn next_change(&mut self) -> Result<(), String> {
        loop {
            let (_, command) =
                protocol::read_command_message(&mut self.reader, self.version).map_err(|e| e.to_string())?;
            if matches!(command, Command::SubscribeEvent(_)) {
                return Ok(());
            }
        }
    }

    pub fn socket(&self) -> std::io::Result<UnixStream> {
        self.reader.get_ref().try_clone()
    }

    fn send(&mut self, command: &Command) -> Result<u32, String> {
        let request = self.next_request;
        self.next_request = self.next_request.wrapping_add(1);
        protocol::write_command_message(self.reader.get_mut(), request, command, self.version)
            .map_err(|e| format!("{}: {e}", name(command)))?;
        Ok(request)
    }

    fn ask<R: CommandReply>(&mut self, command: &Command) -> Result<R, String> {
        let request = self.send(command)?;
        let (answered, reply) = protocol::read_reply_message::<R>(&mut self.reader, self.version)
            .map_err(|e| format!("{}: {e}", name(command)))?;
        same_request(request, answered)?;
        Ok(reply)
    }
}

fn same_request(request: u32, answered: u32) -> Result<(), String> {
    if request == answered {
        Ok(())
    } else {
        Err(format!("the server answered request {answered} while {request} waited"))
    }
}

fn name(command: &Command) -> &'static str {
    match command {
        Command::Auth(_) => "auth",
        Command::SetClientName(_) => "set client name",
        Command::Subscribe(_) => "subscribe",
        Command::GetServerInfo => "server info",
        Command::GetSinkInfoList => "list outputs",
        Command::GetSourceInfoList => "list inputs",
        Command::GetSinkInputInfoList => "list playing apps",
        Command::GetSourceOutputInfoList => "list recording apps",
        _ => "request",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulse_server_names_a_unix_socket_or_nothing() {
        assert_eq!(socket_named("unix:/run/user/1000/pulse/native"), Some("/run/user/1000/pulse/native".into()));
        assert_eq!(socket_named("/tmp/pulse/native"), Some("/tmp/pulse/native".into()));
        assert_eq!(
            socket_named("{4fd0}unix:/run/pulse/native tcp:sound.example:4713"),
            Some("/run/pulse/native".into())
        );
        assert_eq!(socket_named("tcp:sound.example:4713 /tmp/native"), Some("/tmp/native".into()));
        assert_eq!(socket_named("tcp:sound.example:4713"), None);
        assert_eq!(socket_named(""), None);
    }
}
