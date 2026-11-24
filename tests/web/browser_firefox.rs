//! Firefox on a copy of the owner's profile (docs/web-access.md, 「How the browser is started」 and 「The browser's
//! network path」): how it is started off every person's session, what its copy is told, how its page's values
//! are read and when its page has come to rest.

use std::path::Path;

use bmo_explorer::web::browser::detached::DETACHED;
use bmo_explorer::web::browser::firefox::*;
use serde_json::json;

#[test]
fn a_snaps_firefox_starts_in_a_scope_of_its_own_name_and_any_other_without_one() {
  assert_eq!(snap_scope("/snap/bin/firefox"), "snap.firefox.firefox");
  assert_eq!(snap_scope("/snap/bin/browser.nightly"), "snap.browser.nightly");
  assert_eq!(snap_scope("/usr/bin/firefox"), "");
}

#[test]
fn firefox_is_launched_off_every_persons_session_on_the_copy_alone() {
  let copy = Path::new("/srv/profiles/.bmo-web-firefox-1");
  let person = |name: &str| match name {
    "PATH" => Some("/usr/bin".to_owned()),
    "HOME" => Some("/srv/user".to_owned()),
    _ if DETACHED.session_variables.iter().any(|one| one == name) => Some("the person's".to_owned()),
    _ => None,
  };
  let environment =
    launch_environment(person, ":9", &copy.join("display/Xauthority"), &copy.join("display/runtime"), copy);
  let runtime = Path::new("/run/user/1000");
  let line = launch_line("/snap/bin/firefox", "snap.firefox.firefox", "1", runtime, &environment, copy);
  let set: Vec<&str> = line[10..].iter().map(String::as_str).filter(|part| part.contains('=')).collect();

  // The scope is asked of the user's own service manager, whatever the caller's session holds.
  assert_eq!(
    &line[..3],
    [
      "env",
      "XDG_RUNTIME_DIR=/run/user/1000",
      "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus"
    ]
  );
  assert_eq!(
    &line[3..10],
    [
      "systemd-run",
      "--user",
      "--scope",
      "--quiet",
      "--collect",
      "--unit=snap.firefox.firefox-1",
      "--"
    ]
  );
  assert_eq!(&line[10..12], ["env", "-i"]);
  assert!(set.contains(&"DISPLAY=:9"));
  assert!(set.contains(&"DBUS_SESSION_BUS_ADDRESS=unix:path=/srv/profiles/.bmo-web-firefox-1/no-bus"));
  assert!(set.contains(&"DISABLE_WAYLAND=1"));
  assert!(!set.iter().any(|part| part.ends_with("=the person's")));
  assert_eq!(
    &line[line.len() - 6..],
    [
      "--no-remote",
      "--profile",
      "/srv/profiles/.bmo-web-firefox-1",
      "--remote-debugging-port",
      "0",
      "about:blank"
    ]
  );
  assert_eq!(
    launch_line("/usr/bin/firefox", "", "1", runtime, &environment, copy)[..2],
    ["env", "-i"]
  );
}

#[test]
fn a_firefox_that_ends_before_it_listens_is_failed_with_the_last_line_written() {
  assert_eq!(
    ended_before_listening("Failed to connect to bus: No medium found"),
    "Firefox ended before it listened: Failed to connect to bus: No medium found"
  );
  assert_eq!(ended_before_listening(""), "Firefox ended before it listened");
  assert_eq!(
    ended_before_listening(&"x".repeat(1_000)).len(),
    "Firefox ended before it listened: ".len() + 300
  );
}

#[test]
fn the_copy_is_told_to_send_everything_through_the_proxy_and_keep_its_cache_and_downloads_inside() {
  let preferences = start_preferences(4000, Path::new("/srv/profiles/.bmo-web-firefox-1"));

  for line in [
    "user_pref(\"remote.prefs.recommended\", false);",
    "user_pref(\"network.proxy.type\", 1);",
    "user_pref(\"network.proxy.http_port\", 4000);",
    "user_pref(\"network.proxy.ssl_port\", 4000);",
    "user_pref(\"network.proxy.no_proxies_on\", \"\");",
    "user_pref(\"network.proxy.allow_hijacking_localhost\", true);",
    "user_pref(\"network.proxy.failover_direct\", false);",
    "user_pref(\"network.trr.mode\", 5);",
    "user_pref(\"media.peerconnection.ice.proxy_only\", true);",
    "user_pref(\"browser.cache.disk.enable\", false);",
    "user_pref(\"browser.cache.disk.parent_directory\", \"/srv/profiles/.bmo-web-firefox-1/cache\");",
    "user_pref(\"browser.download.folderList\", 2);",
    "user_pref(\"browser.download.dir\", \"/srv/profiles/.bmo-web-firefox-1/downloads\");",
  ] {
    assert!(preferences.lines().any(|written| written == line), "{line}");
  }
}

#[test]
fn a_value_firefox_writes_is_read_as_plain_json() {
  let written = json!({
    "type": "object",
    "value": [
      ["url", { "type": "string", "value": "https://site.example/" }],
      ["links", { "type": "array", "value": [{ "type": "object", "value": [["text", { "type": "string", "value": "a" }]] }] }],
      ["webdriver", { "type": "boolean", "value": false }],
      ["count", { "type": "number", "value": 3 }],
      ["odd", { "type": "number", "value": "NaN" }],
      ["node", { "type": "node", "sharedId": "x" }],
      ["none", { "type": "undefined" }],
    ],
  });

  assert_eq!(
    plain(&written),
    json!({
      "url": "https://site.example/",
      "links": [{ "text": "a" }],
      "webdriver": false,
      "count": 3,
      "odd": null,
      "node": null,
      "none": null,
    })
  );
}

#[test]
fn a_page_has_come_to_rest_once_loaded_with_its_network_quiet_for_half_a_second() {
  let quiet = Quiet::default();

  quiet.begun("1", 1_000);
  assert!(!quiet.rested(true, 5_000));

  quiet.ended("1", 1_200);
  assert!(!quiet.rested(true, 1_600));
  assert!(quiet.rested(true, 1_700));
  assert!(!quiet.rested(false, 9_000));
}
