#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! The signature tables hold everything in the API definition.

use psi_ext_api::{EVENTS, FUNCTIONS, TYPES};

const API: &str = concat!(
    include_str!("../api/psi.toml"),
    include_str!("../api/std.toml")
);

fn count(header: &str) -> usize {
    API.lines().filter(|l| l.trim() == header).count()
}

#[test]
fn counts_match_the_toml() {
    assert_eq!(FUNCTIONS.len(), count("[[function]]"));
    assert_eq!(EVENTS.len(), count("[[event]]"));
    assert_eq!(TYPES.len(), count("[[type]]"));
    // Today's API, so a parsing slip that drops items shows up.
    assert_eq!((FUNCTIONS.len(), EVENTS.len(), TYPES.len()), (62, 19, 40));
}

#[test]
fn every_name_in_the_toml_is_in_a_table() {
    let mut section = "";
    for line in API.lines().map(str::trim) {
        if line.starts_with("[[") {
            section = line;
        }
        let Some(name) = line
            .strip_prefix("name = \"")
            .and_then(|r| r.strip_suffix('"'))
        else {
            continue;
        };
        let found = match section {
            "[[function]]" => FUNCTIONS.iter().any(|f| f.name == name),
            "[[event]]" => EVENTS.iter().any(|e| e.name == name),
            "[[type]]" => TYPES.iter().any(|t| t.name == name),
            other => panic!("name outside a known section: {other}"),
        };
        assert!(found, "{section} {name} is missing");
    }
}

#[test]
fn details_are_kept() {
    let confirm = FUNCTIONS.iter().find(|f| f.name == "ui.confirm").unwrap();
    assert!(confirm.is_async);
    assert_eq!(confirm.returns, Some("boolean"));
    assert_eq!(
        confirm.params.iter().map(|p| p.name).collect::<Vec<_>>(),
        ["title", "message"]
    );
    let notify = FUNCTIONS.iter().find(|f| f.name == "ui.notify").unwrap();
    assert!(notify.params[1].optional && !notify.params[0].optional);
    let gate = EVENTS.iter().find(|e| e.name == "tool_call").unwrap();
    assert_eq!((gate.kind, gate.result), ("gate", Some("ToolCallResult?")));
    let alias = TYPES.iter().find(|t| t.name == "EventName").unwrap();
    assert_eq!(alias.alias, Some("events"));
}
