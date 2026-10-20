use std::path::PathBuf;

use bmo_explorer::tool::{Grant, Session};
use serde_json::json;

#[test]
fn a_record_reads_as_the_tool_host_hands_it() {
  let record: Session = serde_json::from_value(json!({
    "holderName": "search",
    "namespaces": ["web"],
    "capabilities": ["web.read", "web.images"],
    "grants": [{ "resource": "notes" }],
    "shown": ["web/web_search"],
    "variables": { "BMO_DATA_DIR": "/data" },
    "variablesFile": "/run/variables.json",
    "labels": { "research": { "closesAt": 1_700_000_000_000_u64 }, "owner": "tester" },
    "expiresAt": 1_700_000_100_000_u64,
    "createdAt": 1_700_000_000_000_u64,
  }))
  .unwrap();

  assert!(record.holds(&["web.read", "web.images"]));
  assert!(!record.holds(&["web.read", "web.branches"]));
  assert_eq!(
    record.grants,
    [Grant {
      resource: "notes".into()
    }]
  );
  assert_eq!(record.data_root(), Ok(PathBuf::from("/data")));
  assert_eq!(record.label("owner"), Some(&json!("tester")));
  assert_eq!(
    serde_json::from_value::<Session>(serde_json::to_value(&record).unwrap()).unwrap(),
    record
  );

  // A record that stands until revoked names no lapse, and one shown nothing by name lists nothing.
  let standing: Session = serde_json::from_value(json!({
    "holderName": "worker", "namespaces": [], "capabilities": [], "grants": [], "expiresAt": null, "createdAt": 1,
  }))
  .unwrap();

  assert!(standing.expires_at.is_none() && standing.shown.is_empty() && standing.labels.is_empty());
}

#[test]
fn a_field_the_record_does_not_know_is_refused() {
  let read = serde_json::from_value::<Session>(json!({
    "holderName": "worker", "namespaces": [], "capabilities": [], "grants": [], "createdAt": 1, "dataRoot": "/data",
  }));

  assert!(read.unwrap_err().to_string().contains("dataRoot"));
}
