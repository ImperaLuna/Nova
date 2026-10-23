//! Property tests for `decode_request`, the parser every line on novad's
//! socket goes through. Lines come from any process the user runs, so random
//! and hostile input must only ever give an error, and the valid requests
//! that do decode must be exactly what a client meant to send.

use nova_protocol::{
    AiProvider, Command, PROTOCOL_VERSION, Request, Step, Topic, UsageLimit, UsageWindow, decode_request, encode,
};
use proptest::prelude::*;
use serde_json::{Map, Value, json};

/// The fields a request line may have; anything else is refused.
const LINE_FIELDS: [&str; 4] = ["v", "id", "cmd", "args"];

/// Names that appear in real requests, so generated JSON hits the decoder's
/// paths past the first unknown field.
const NAMES: &[&str] = &[
    "v",
    "id",
    "cmd",
    "args",
    "ping",
    "list_features",
    "subscribe",
    "status",
    "quit",
    "show_ai_usage",
    "report_claude_usage",
    "focus_workspace",
    "step_workspace",
    "topics",
    "provider",
    "limits",
    "window",
    "percent",
    "resets_at",
    "refresh_ai_usage",
    "output",
    "step",
    "next",
    "claude",
    "weekly",
    "logs",
];

fn topic() -> impl Strategy<Value = Topic> {
    prop_oneof![
        Just(Topic::Features),
        Just(Topic::Logs),
        Just(Topic::Theme),
        Just(Topic::AiUsage),
        Just(Topic::Compositor),
    ]
}

fn usage_limit() -> impl Strategy<Value = UsageLimit> {
    (
        prop_oneof![Just(UsageWindow::Session), Just(UsageWindow::Weekly)],
        any::<u8>(),
        proptest::option::of(any::<String>()),
    )
        .prop_map(|(window, percent, resets_at)| UsageLimit { window, percent, resets_at })
}

fn command() -> impl Strategy<Value = Command> {
    prop_oneof![
        Just(Command::Ping),
        Just(Command::ListFeatures),
        Just(Command::Status),
        Just(Command::Quit),
        Just(Command::RefreshAiUsage),
        proptest::collection::vec(topic(), 0..6).prop_map(|topics| Command::Subscribe { topics }),
        prop_oneof![Just(AiProvider::Claude), Just(AiProvider::Codex)]
            .prop_map(|provider| Command::ShowAiUsage { provider }),
        proptest::collection::vec(usage_limit(), 0..4).prop_map(|limits| Command::ReportClaudeUsage { limits }),
        any::<i64>().prop_map(|id| Command::FocusWorkspace { id }),
        (any::<String>(), prop_oneof![Just(Step::Next), Just(Step::Previous)])
            .prop_map(|(output, step)| Command::StepWorkspace { output, step }),
    ]
}

fn request() -> impl Strategy<Value = Request> {
    (proptest::option::of(any::<u64>()), command()).prop_map(|(id, command)| Request { id, command })
}

/// Any JSON, with keys and strings mostly drawn from [`NAMES`].
fn json() -> impl Strategy<Value = Value> {
    let name = || prop_oneof![3 => proptest::sample::select(NAMES).prop_map(String::from), 1 => any::<String>()];
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        any::<u64>().prop_map(Value::from),
        any::<f64>().prop_map(Value::from),
        name().prop_map(Value::from),
    ];
    leaf.prop_recursive(4, 32, 6, move |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..6).prop_map(Value::from),
            proptest::collection::btree_map(name(), inner, 0..6)
                .prop_map(|fields| Value::Object(fields.into_iter().collect())),
        ]
    })
}

/// A request-shaped object: the line's own fields, each one sometimes left
/// out, with any JSON as the values.
fn request_shaped() -> impl Strategy<Value = Value> {
    (
        proptest::option::of(prop_oneof![4 => Just(json!(PROTOCOL_VERSION)), 1 => json()]),
        proptest::option::of(prop_oneof![any::<u64>().prop_map(Value::from), json()]),
        proptest::option::of(prop_oneof![4 => proptest::sample::select(NAMES).prop_map(Value::from), 1 => json()]),
        proptest::option::of(json()),
    )
        .prop_map(|(v, id, cmd, args)| {
            let fields = LINE_FIELDS.into_iter().zip([v, id, cmd, args]);
            Value::Object(fields.filter_map(|(key, value)| Some((key.to_owned(), value?))).collect())
        })
}

/// What decoding `line` must give if it gives anything: the same request
/// again when re-encoded, from a line with only the allowed fields.
fn check_accepted(line: &str) -> Result<(), TestCaseError> {
    if let Ok(request) = decode_request(line) {
        let fields: Map<String, Value> = serde_json::from_str(line).expect("an accepted line is a JSON object");
        prop_assert!(fields.keys().all(|key| LINE_FIELDS.contains(&key.as_str())), "{line}");
        prop_assert_eq!(&fields["v"], &json!(PROTOCOL_VERSION));
        prop_assert_eq!(decode_request(&encode(&request).unwrap()), Ok(request));
    }
    Ok(())
}

proptest! {
    #[test]
    fn any_text_is_refused_or_a_real_request(line in any::<String>()) {
        check_accepted(&line)?;
    }

    #[test]
    fn any_json_is_refused_or_a_real_request(value in prop_oneof![json(), request_shaped()]) {
        check_accepted(&value.to_string())?;
    }

    #[test]
    fn every_request_round_trips(request in request()) {
        let line = encode(&request).unwrap();
        prop_assert_eq!(decode_request(&line), Ok(request));
    }

    #[test]
    fn extra_fields_are_refused(request in request(), key in any::<String>(), value in json()) {
        prop_assume!(!LINE_FIELDS.contains(&key.as_str()));
        let mut fields: Map<String, Value> = serde_json::from_str(&encode(&request).unwrap()).unwrap();
        fields.insert(key, value);
        prop_assert!(decode_request(&Value::Object(fields).to_string()).is_err());
    }

    #[test]
    fn other_versions_are_refused(request in request(), v in any::<u32>()) {
        prop_assume!(v != PROTOCOL_VERSION);
        let mut fields: Map<String, Value> = serde_json::from_str(&encode(&request).unwrap()).unwrap();
        fields.insert("v".into(), v.into());
        prop_assert!(decode_request(&Value::Object(fields).to_string()).is_err());
    }

    #[test]
    fn cut_off_lines_are_refused(request in request(), cut in any::<proptest::sample::Index>()) {
        let line = encode(&request).unwrap();
        let line = line.trim_end();
        let mut end = cut.index(line.len());
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        prop_assert!(decode_request(&line[..end]).is_err());
    }
}

/// Nesting deep enough to overflow a recursive parser's stack.
#[test]
fn deep_nesting_is_refused() {
    let depth = 100_000;
    let nested = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
    assert!(decode_request(&nested).is_err());
    assert!(decode_request(&format!(r#"{{"v":{PROTOCOL_VERSION},"cmd":"ping","args":{nested}}}"#)).is_err());
}
