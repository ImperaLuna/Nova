use nova_carrier::names::tagged_names_of;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;
use std::rc::Rc;

const EVENT: &str = "event";
const DATA: &str = "data";

type Handler<C> = Rc<dyn Fn(&Value, &mut C)>;

pub(crate) struct Routes<C> {
    next_id: u64,
    by_event: HashMap<&'static str, Vec<(u64, Handler<C>)>>,
}

impl<C> Default for Routes<C> {
    fn default() -> Self {
        Self { next_id: 0, by_event: HashMap::new() }
    }
}

impl<C> Routes<C> {
    pub(crate) fn add<E: DeserializeOwned + 'static>(&mut self, handler: impl Fn(E, &mut C) + 'static) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        let handler: Handler<C> = Rc::new(move |event, context| match E::deserialize(event) {
            Ok(event) => handler(event, context),
            Err(e) => crate::log::warn(&format!("can't read novad's {} event: {e}", event[EVENT])),
        });
        for name in tagged_names_of::<E>(EVENT) {
            self.by_event.entry(name).or_default().push((id, Rc::clone(&handler)));
        }
        id
    }

    pub(crate) fn remove(&mut self, id: u64) {
        for handlers in self.by_event.values_mut() {
            handlers.retain(|(each, _)| *each != id);
        }
    }

    pub(crate) fn of(&self, name: &str, data: Value) -> (Value, Vec<Handler<C>>) {
        let handlers = self.by_event.get(name).into_iter().flatten().map(|(_, handler)| Rc::clone(handler)).collect();
        let mut event = serde_json::Map::new();
        event.insert(EVENT.into(), name.into());
        event.insert(DATA.into(), data);
        (event.into(), handlers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(tag = "event", content = "data", rename_all = "snake_case")]
    enum Weather {
        Rain { millimetres: u32 },
        Sun,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(tag = "event", content = "data", rename_all = "snake_case")]
    enum Tide {
        High,
    }

    fn send(routes: &Routes<Vec<String>>, name: &str, data: Value, seen: &mut Vec<String>) {
        let (event, handlers) = routes.of(name, data);
        for handler in handlers {
            handler(&event, seen);
        }
    }

    #[test]
    fn a_handler_gets_its_types_events_only() {
        let mut routes = Routes::default();
        routes.add(|event: Weather, seen: &mut Vec<String>| seen.push(format!("{event:?}")));
        routes.add(|event: Tide, seen: &mut Vec<String>| seen.push(format!("{event:?}")));
        let mut seen = Vec::new();
        send(&routes, "rain", serde_json::json!({"millimetres": 4}), &mut seen);
        send(&routes, "sun", Value::Null, &mut seen);
        send(&routes, "high", Value::Null, &mut seen);
        send(&routes, "snow", Value::Null, &mut seen);
        assert_eq!(seen, ["Rain { millimetres: 4 }", "Sun", "High"]);
    }

    #[test]
    fn an_event_it_cant_read_is_skipped() {
        let mut routes = Routes::default();
        routes.add(|event: Weather, seen: &mut Vec<String>| seen.push(format!("{event:?}")));
        let mut seen = Vec::new();
        send(&routes, "rain", serde_json::json!({"millimetres": "a lot"}), &mut seen);
        assert!(seen.is_empty(), "{seen:?}");
    }

    #[test]
    fn a_removed_handler_gets_nothing() {
        let mut routes = Routes::default();
        let id = routes.add(|event: Weather, seen: &mut Vec<String>| seen.push(format!("{event:?}")));
        routes.remove(id);
        let mut seen = Vec::new();
        send(&routes, "sun", Value::Null, &mut seen);
        assert!(seen.is_empty(), "{seen:?}");
    }
}
