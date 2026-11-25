use super::*;

fn decode_request(line: &str) -> Result<Request<Command>, String> {
    nova_carrier::decode_request(line)
}

#[test]
fn colors_are_six_digit_hex() {
    assert_eq!("#fd7014".parse::<Color>().unwrap().rgb(), [0xFD, 0x70, 0x14]);
    assert_eq!(String::from("#fd7014".parse::<Color>().unwrap()), "#FD7014");
    for bad in ["fd7014", "#fd701", "#fd70144", "#fd70zz", "#ffffffff", "#ééé"] {
        assert!(bad.parse::<Color>().is_err(), "{bad} parsed");
    }
}

#[test]
fn subscribe_parses_with_topics() {
    let req = decode_request(r#"{"v":2,"id":1,"cmd":"subscribe","args":{"topics":["features","logs"]}}"#).unwrap();
    assert_eq!(req.command, Command::Subscribe { topics: vec!["features".into(), "logs".into()] });
}

#[test]
fn panel_names_parse_and_match_the_wire() {
    for panel in Panel::ALL {
        assert_eq!(panel.name().parse::<Panel>(), Ok(panel));
        assert_eq!(serde_json::to_value(panel).unwrap(), json!(panel.name()));
    }
    assert!(
        "calculator"
            .parse::<Panel>()
            .unwrap_err()
            .contains("known: ai-usage, power, quick-settings, launcher, tray, media, wallpaper")
    );
    let features = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../features");
    for panel in Panel::ALL {
        assert!(features.join(panel.name()).join("feature.toml").is_file(), "{panel} has no feature");
    }
}

#[test]
fn the_log_feed_is_the_logs_event() {
    let logs = Logs { backlog: true, entries: vec![log_entry()] };
    assert_eq!(encode(logs.event()).unwrap(), encode(Event::Logs(logs)).unwrap());
}

#[test]
fn the_desktop_is_the_compositor_event() {
    let desktop = desktop();
    let event = nova_desktop_info::Event::Compositor(desktop.clone());
    assert_eq!(encode(nova_compositor::event(&desktop)).unwrap(), encode(event).unwrap());
}

#[test]
fn only_hello_has_no_topic() {
    assert_eq!(Event::Hello { daemon: "x".into() }.topic(), None);
    assert_eq!(Event::Features { features: vec![] }.topic(), Some(topics::FEATURES));
    assert_eq!(Event::Logs(Logs { backlog: false, entries: vec![] }).topic(), Some(topics::LOGS));
    assert_eq!(nova_theme_info::Event::Theme(theme()).topic(), Some(nova_theme_info::TOPIC));
    assert_eq!(nova_desktop_info::Event::Compositor(desktop()).topic(), Some(nova_desktop_info::TOPIC));
    let toggle = nova_bar_api::Event::Toggle { panel: Panel::AiUsage, output: None };
    assert_eq!(toggle.topic(), Some(nova_bar_api::TOPIC));
    assert_eq!(nova_volume_osd_api::Event::Volume { key: VolumeKey::Up }.topic(), Some(nova_volume_osd_api::TOPIC));
    assert_eq!(nova_host_info::Event::Host(host()).topic(), Some(nova_host_info::TOPIC));
}

#[test]
fn unknown_argument_fields_are_rejected() {
    let error = decode_request(r#"{"v":2,"cmd":"subscribe","args":{"topics":["logs"],"unexpected":true}}"#);
    assert!(error.unwrap_err().contains("unknown field `unexpected`"));
    assert!(decode_request(r#"{"v":2,"cmd":"ping","args":{"unexpected":true}}"#).is_err());
    let limit = r#"{"window":"weekly","percent":5,"extra":1}"#;
    let line = format!(r#"{{"v":2,"cmd":"report_claude_usage","args":{{"limits":[{limit}]}}}}"#);
    assert!(nova_carrier::decode_request::<nova_ai_usage_api::Command>(&line).is_err());
    let rect = r#"{"x":0,"y":0,"width":1,"height":1,"scale":2}"#;
    let line = format!(r#"{{"v":2,"cmd":"save_capture","args":{{"output":"DP-1","rect":{rect}}}}}"#);
    assert!(nova_carrier::decode_request::<nova_screenshot_api::Command>(&line).is_err());
}

#[test]
fn request_without_args_parses() {
    let req = decode_request(r#"{"v":2,"id":7,"cmd":"ping"}"#).unwrap();
    assert_eq!(req, Request { id: Some(7), command: Command::Ping });
}

#[test]
fn request_without_id_parses() {
    let req = decode_request(r#"{"v":2,"cmd":"list_features"}"#).unwrap();
    assert_eq!(req.id, None);
    assert_eq!(req.command, Command::ListFeatures);
}

#[test]
fn unknown_command_is_rejected() {
    assert!(decode_request(r#"{"v":2,"cmd":"reboot"}"#).is_err());
}

#[test]
fn unknown_fields_are_rejected() {
    assert!(decode_request(r#"{"v":2,"id":1,"cmd":"ping"}"#).is_ok());
    assert!(decode_request(r#"{"v":2,"id":1,"cmd":"ping","extra":true}"#).is_err());
    assert!(decode_request(r#"{"v":2,"cmd":"ping","args":{"x":1}}"#).is_err());
}

#[test]
fn host_rejects_unknown_fields() {
    fn with_extra(value: serde_json::Value, pointer: &str) -> serde_json::Value {
        let mut value = value;
        value.pointer_mut(pointer).unwrap().as_object_mut().unwrap().insert("extra".into(), 1.into());
        value
    }
    let host = serde_json::to_value(host()).unwrap();
    assert!(serde_json::from_value::<Host>(host.clone()).is_ok());
    for pointer in ["", "/os"] {
        assert!(serde_json::from_value::<Host>(with_extra(host.clone(), pointer)).is_err(), "{pointer}");
    }
}

#[test]
fn other_protocol_versions_are_rejected() {
    let error = decode_request(r#"{"v":1,"id":1,"cmd":"ping"}"#).unwrap_err();
    assert!(error.contains("version 1"), "{error}");
    assert!(decode_request(r#"{"id":1,"cmd":"ping"}"#).is_err());
}

#[test]
fn responses_round_trip() {
    for outcome in [Outcome::Result(json!("pong")), Outcome::Error("nope".into())] {
        let msg = Envelope::new(Response { id: 3, outcome });
        let back: Envelope<Response> = serde_json::from_str(&serde_json::to_string(&msg).unwrap()).unwrap();
        assert_eq!(back, msg);
    }
}
