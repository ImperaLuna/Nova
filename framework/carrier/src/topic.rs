use serde::Serialize;
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Topic(&'static str);

impl Topic {
    pub const fn named(name: &'static str) -> Self {
        Self(name)
    }

    pub fn name(self) -> &'static str {
        self.0
    }
}

impl fmt::Debug for Topic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Topic({})", self.0)
    }
}

pub trait Published: Serialize {
    fn topic(&self) -> Option<Topic>;
}

#[derive(Serialize)]
pub struct Single<'a, T: Serialize> {
    #[serde(rename = "event")]
    name: &'static str,
    data: &'a T,
    #[serde(skip)]
    topic: Topic,
}

impl<'a, T: Serialize> Single<'a, T> {
    pub fn new(name: &'static str, topic: Topic, data: &'a T) -> Self {
        Self { name, data, topic }
    }
}

impl<T: Serialize> Published for Single<'_, T> {
    fn topic(&self) -> Option<Topic> {
        Some(self.topic)
    }
}
