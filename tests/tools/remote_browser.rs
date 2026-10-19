use std::path::Path;

use bmo_explorer::tool::manifest::Manifest;
use bmo_explorer::tools::launching::Turn;
use bmo_explorer::tools::remote_browser::{RemoteBrowserInput, TOOLS, launch};
use serde_json::json;

#[test]
fn the_manifest_declares_the_entry_as_its_tool_does() {
  let manifest =
    Manifest::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/remote_browser"), "remote_browser")
      .unwrap();

  assert_eq!(manifest.differences(TOOLS), Vec::<String>::new());
}

#[test]
fn a_remote_browser_goes_sync_by_default_with_its_steps_file_and_its_secrets_on_standard_input() {
  let mut turn = Turn::default();

  turn.variables.insert("BMO_TURN_CHAT".into(), "-10".into());

  let input: RemoteBrowserInput = serde_json::from_value(json!({
    "node": "desk",
    "plan": "[{\"do\":\"quit\"}]",
    "secrets": "PASSWORD=hunter2",
  }))
  .unwrap();
  let launch = launch(&turn, &input, Path::new("/tmp/steps.json")).unwrap();

  assert_eq!(
    launch.args,
    [
      "remote-browser",
      "--chat",
      "-10",
      "--by",
      "chat:-10",
      "--mode",
      "sync",
      "--node",
      "desk",
      "--steps",
      "/tmp/steps.json"
    ]
  );
  assert_eq!(launch.input, "PASSWORD=hunter2");
  assert!(
    !launch.args.iter().any(|word| word.contains("hunter2")),
    "a secret never rides as an argument"
  );
}
