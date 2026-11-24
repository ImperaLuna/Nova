use nova_carrier::{Outcome, RawRequest, Response, encode};
use nova_hub::{ClientId, ClientQueue, Outgoing};
use nova_log::Logger;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::sync::{Arc, mpsc};

const LOG_SOURCE: &str = "server";

type SlowWork = Box<dyn FnOnce() -> Outcome + Send>;

pub enum Answer {
    Now(Outcome),
    Later(SlowWork),
}

impl Answer {
    pub fn later(work: impl FnOnce() -> Outcome + Send + 'static) -> Self {
        Self::Later(Box::new(work))
    }
}

pub fn done(result: Result<(), String>) -> Outcome {
    value(result)
}

pub fn value(result: Result<impl Serialize, String>) -> Outcome {
    match result.and_then(|value| serde_json::to_value(value).map_err(|e| e.to_string())) {
        Ok(value) => Outcome::Result(value),
        Err(e) => Outcome::Error(e),
    }
}

pub struct Caller {
    id: ClientId,
    peer_pid: Option<u32>,
    queue: ClientQueue,
    logger: Arc<Logger>,
}

impl Caller {
    pub(crate) fn new(id: ClientId, peer_pid: Option<u32>, queue: ClientQueue, logger: Arc<Logger>) -> Self {
        Self { id, peer_pid, queue, logger }
    }

    pub fn id(&self) -> ClientId {
        self.id
    }

    pub fn peer_pid(&self) -> Option<u32> {
        self.peer_pid
    }

    pub fn send(&self, message: impl Serialize) {
        match encode(message) {
            Ok(line) => {
                self.queue.push(Outgoing::Line(line));
            }
            Err(e) => self.logger.error(LOG_SOURCE, &format!("can't encode a message: {e}")),
        }
    }

    pub fn flush(&self) {
        let (done, flushed) = mpsc::sync_channel(1);
        if self.queue.push(Outgoing::Flushed(done)) {
            let _ = flushed.recv();
        }
    }

    fn respond(&self, id: Option<u64>, answer: Answer) {
        match answer {
            Answer::Now(outcome) => {
                if let Some(id) = id {
                    self.send(Response { id, outcome });
                }
            }
            Answer::Later(work) => self.answer_later(id, work),
        }
    }

    fn answer_later(&self, id: Option<u64>, work: SlowWork) {
        let queue = self.queue.clone();
        let logger = Arc::clone(&self.logger);
        std::thread::spawn(move || {
            let outcome = work();
            let Some(id) = id else { return };
            match encode(Response { id, outcome }) {
                Ok(line) => {
                    queue.push(Outgoing::Line(line));
                }
                Err(e) => logger.error(LOG_SOURCE, &format!("can't encode a message: {e}")),
            }
        });
    }
}

type Route<S> = Box<dyn Fn(&S, &Caller, RawRequest) -> Result<(), String> + Send + Sync>;

pub struct Router<S> {
    routes: Vec<Route<S>>,
    by_name: HashMap<&'static str, usize>,
    names: &'static [&'static str],
}

impl<S> Default for Router<S> {
    fn default() -> Self {
        Self { routes: Vec::new(), by_name: HashMap::new(), names: &[] }
    }
}

impl<S: 'static> Router<S> {
    pub fn new() -> Self {
        Self::default()
    }

    #[expect(clippy::panic, reason = "two owners of one command is a build mistake the app's router test catches")]
    pub fn route<C: DeserializeOwned + 'static>(
        mut self,
        answer: impl Fn(&S, &Caller, &C) -> Answer + Send + Sync + 'static,
        then: impl Fn(&S, &Caller, C) + Send + Sync + 'static,
    ) -> Self {
        let names = nova_carrier::names::tagged_names_of::<C>("cmd");
        for name in names {
            let index = self.routes.len();
            if self.by_name.insert(name, index).is_some() {
                panic!("two message types both take the command {name:?}");
            }
        }
        self.names = match self.routes.is_empty() {
            true => names,
            false => Box::leak([self.names, names].concat().into_boxed_slice()),
        };
        self.routes.push(Box::new(move |service, caller, raw| {
            let request = raw.decode::<C>()?;
            caller.respond(request.id, answer(service, caller, &request.command));
            then(service, caller, request.command);
            Ok(())
        }));
        self
    }

    pub fn names(&self) -> &'static [&'static str] {
        self.names
    }

    pub(crate) fn handle(&self, service: &S, caller: &Caller, raw: RawRequest) -> Result<(), String> {
        let Some(&index) = self.by_name.get(raw.cmd.as_str()) else {
            return Err(<serde_json::Error as serde::de::Error>::unknown_variant(&raw.cmd, self.names).to_string());
        };
        self.routes[index](service, caller, raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
    enum Greetings {
        Hello,
        Wave,
    }

    #[derive(Deserialize)]
    #[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
    enum Farewells {
        Bye,
    }

    #[derive(Deserialize)]
    #[serde(tag = "cmd", content = "args", rename_all = "snake_case")]
    enum Clash {
        Hello,
    }

    fn router() -> Router<()> {
        Router::new()
            .route::<Greetings>(|_, _, _| Answer::Now(done(Ok(()))), |_, _, _| {})
            .route::<Farewells>(|_, _, _| Answer::Now(done(Ok(()))), |_, _, _| {})
    }

    #[test]
    fn each_type_brings_its_names() {
        assert_eq!(router().names(), ["hello", "wave", "bye"]);
    }

    #[test]
    #[should_panic(expected = "two message types both take the command \"hello\"")]
    fn a_name_has_one_route() {
        let _ = router().route::<Clash>(|_, _, _| Answer::Now(done(Ok(()))), |_, _, _| {});
    }

    #[test]
    fn an_unknown_name_lists_every_name() {
        #[derive(Debug, Deserialize)]
        #[serde(tag = "cmd", content = "args", rename_all = "snake_case")]
        enum All {
            Hello,
            Wave,
            Bye,
        }
        let expected = serde_json::from_value::<All>(serde_json::json!({"cmd": "dance"})).unwrap_err().to_string();
        let error = <serde_json::Error as serde::de::Error>::unknown_variant("dance", router().names()).to_string();
        assert_eq!(error, expected);
    }
}
