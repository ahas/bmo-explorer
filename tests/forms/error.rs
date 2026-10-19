use serde_json::json;
use serde_json::to_value;

use bmo_explorer::forms::error::{Failure, WebError, WebResult};

// A failure is written by its snake-case name, as the helper's socket and a tool's answer have it.
#[test]
fn a_failure_is_written_by_its_snake_case_name() {
  assert_eq!(to_value(Failure::UnsafeAddress).unwrap().as_str(), Some("unsafe_address"));
}

// A result is written as Ok or Err, the one form the web layer and a placed tool share.
#[test]
fn a_result_is_written_as_ok_or_err() {
  let answered: WebResult<String> = Ok("words".into());
  let failed: WebResult<String> = Err(WebError::new("gone", Failure::Http));

  assert_eq!(to_value(answered).unwrap(), json!({"Ok": "words"}));
  assert_eq!(
    to_value(failed).unwrap(),
    json!(
      {"Err": {"message": "gone", "kind": "http", "status": null, "url": null, "chrome_fallback": false}}
    ),
  );
}
