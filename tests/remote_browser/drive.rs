use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bmo_explorer::remote_browser::*;
use bmo_explorer::settings::RemoteBrowser;
use serde_json::value::RawValue;
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;

// The port and the profile a plan names neither of, as a deployment's settings name them.
fn defaults() -> RemoteBrowser {
  RemoteBrowser {
    port: 9222,
    profile: "default-profile".into(),
  }
}

// Whether a program of this name stands on the PATH.
fn on_path(name: &str) -> bool {
  std::env::var_os("PATH")
    .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
}

fn plan(steps: &[Value]) -> Plan {
  Plan {
    port: 9222,
    profile: "default-profile".into(),
    steps: steps.iter().map(|step| RawValue::from_string(step.to_string()).unwrap()).collect(),
  }
}

fn secrets(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
  pairs.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
}

fn exited(code: i64, stderr: &str) -> ExecEnd {
  ExecEnd::Exit {
    code: Some(code),
    stderr: stderr.into(),
  }
}

// A node that answers each exec with the lines and the ending given for it, in turn, and keeps what each
// exec was handed.
struct Answers {
  answers: Vec<(Vec<Value>, ExecEnd)>,
  asked: RefCell<Vec<BTreeMap<String, String>>>,
}

impl Exec for Answers {
  async fn exec(&self, _: &str, env: &BTreeMap<String, String>, lines: UnboundedSender<String>) -> ExecEnd {
    let turn = self.asked.borrow().len().min(self.answers.len() - 1);
    let (said, end) = &self.answers[turn];

    self.asked.borrow_mut().push(env.clone());

    for line in said {
      lines.send(line.to_string()).unwrap();
    }

    end.clone()
  }
}

#[derive(Default)]
struct Kept {
  shots: Vec<String>,
  notes: Vec<String>,
}

impl Record for Kept {
  fn keep_shot(&mut self, index: i64, name: &str, format: &str, bytes: &[u8]) -> String {
    self.shots.push(format!("{index}:{name}:{format}:{}", String::from_utf8_lossy(bytes)));

    format!("/shots/{name}.jpg")
  }

  async fn note(&mut self, text: String) {
    self.notes.push(text);
  }
}

fn helper() -> Helper {
  Helper {
    code: "code".into(),
    hash: "abc".into(),
  }
}

fn refusal(text: &str) -> String {
  parse_plan(text, &defaults()).expect_err("the plan was taken")
}

#[test]
fn the_steps_are_a_list_alone_or_an_object_with_the_browsers_port_and_profile_beside_it() {
  let alone = parse_plan(r#"[{"do":"goto","url":"https://a.example"}]"#, &defaults()).unwrap();

  assert_eq!((alone.port, alone.profile.as_str(), alone.steps.len()), (9222, "default-profile", 1));
  assert_eq!(alone.steps[0].get(), r#"{"do":"goto","url":"https://a.example"}"#);

  let held = parse_plan(r#"{"port":9333,"profile":"check","steps":[{"do":"quit"}]}"#, &defaults()).unwrap();

  assert_eq!((held.port, held.profile.as_str(), held.steps.len()), (9333, "check", 1));
}

#[test]
fn the_steps_are_refused_whole_naming_the_step_and_what_is_wrong_with_it() {
  assert!(refusal("not json").starts_with("the steps are not JSON"));
  assert_eq!(refusal("[]"), "there are no steps");
  assert!(refusal(r#"[{"do":"fly"}]"#).starts_with(r#"step 1: "do" must be one of"#));
  assert_eq!(
    refusal(r#"[{"do":"key","key":"Enter"},{"do":"goto","url":"x","wait":1}]"#),
    r#"step 2: a goto step takes no "wait""#
  );
  assert_eq!(
    refusal(r#"[{"do":"type","text":"a","secret":"PW"}]"#),
    r#"step 1: a type step carries either "text" or "secret""#
  );
  assert_eq!(
    refusal(r#"[{"do":"type","secret":"pw"}]"#),
    "step 1: a secret's name is capitals, digits and underscores"
  );
  assert_eq!(
    refusal(r#"[{"do":"click","x":3}]"#),
    r#"step 1: a click by point needs both "x" and "y""#
  );
  assert_eq!(
    refusal(r##"[{"do":"click","selector":"#a","text":"b"}]"##),
    r#"step 1: a click names one of "selector", "text" or "x" and "y""#
  );
  assert_eq!(
    refusal(r#"[{"do":"wait","ms":1,"url":"a"}]"#),
    "step 1: a wait step waits for one thing"
  );
  assert_eq!(refusal(r#"[{"do":"read","js":1}]"#), r#"step 1: "js" must be a string"#);
  assert_eq!(
    refusal(r#"{"port":80,"steps":[{"do":"quit"}]}"#),
    r#""port" is a whole number from 1024 to 65535"#
  );
  assert_eq!(
    refusal(r#"{"profile":"../x","steps":[{"do":"quit"}]}"#),
    r#""profile" is letters, digits, hyphens and underscores"#
  );
  assert_eq!(refusal(r#"{"steps":[{"do":"quit"}],"secrets":{}}"#), r#"the plan takes no "secrets""#);
  assert_eq!(
    refusal(&json!([{ "do": "read", "js": "x".repeat(100_000) }]).to_string()),
    "the steps come to more than 96 KiB; split them into runs"
  );
}

#[test]
fn the_secrets_are_read_out_of_an_environment_file_keeping_only_the_names_the_steps_use() {
  let text = "# a comment\nOTHER=kept-out\nexport ID='hana'\nPW=s3cret\n";
  let names = ["ID".to_owned(), "PW".to_owned()];

  assert_eq!(read_secrets(text, &names).unwrap(), secrets(&[("ID", "hana"), ("PW", "s3cret")]));
  assert_eq!(
    read_secrets(text, &["ID".to_owned(), "NONE".to_owned()]).unwrap_err(),
    "the steps name NONE, and standard input gave no value for it"
  );
  assert_eq!(
    secret_names(
      &parse_plan(
        r##"[{"do":"type","selector":"#a","secret":"PW"},{"do":"type","selector":"#b","secret":"PW"}]"##,
        &defaults()
      )
      .unwrap()
    ),
    ["PW"]
  );
}

// Carried from packages/web/remote-browser.test.ts:
// never reach what the run writes, whatever echoed them
#[test]
fn never_reach_what_the_run_writes_whatever_echoed_them() {
  assert_eq!(
    redact("user hana typed s3cret! twice: s3cret!", &secrets(&[("PW", "s3cret!"), ("ID", "hana")])),
    "user ‹ID› typed ‹PW› twice: ‹PW›"
  );
}

// Carried from packages/web/remote-browser.test.ts:
// writes the helper only where the node lacks it, carries a pause into the next exec, and keeps what came back
#[tokio::test]
async fn writes_the_helper_only_where_the_node_lacks_it_and_carries_a_pause_into_the_next_exec() {
  let plan = plan(&[
    json!({ "do": "type", "selector": "#pw", "secret": "PW" }),
    json!({ "do": "shot" }),
    json!({ "do": "read", "js": "1" }),
  ]);
  let node = Answers {
    answers: vec![
      (vec![], exited(HELPER_MISSING, "")),
      (
        vec![
          json!({ "e": "browser", "launched": true, "product": "Edg/1" }),
          json!({ "e": "step", "i": 0, "do": "type", "detail": "typed hunter22 into #pw", "url": "https://a" }),
          json!({ "e": "pause", "next": 1, "target": "T1" }),
        ],
        exited(0, ""),
      ),
      (
        vec![
          json!({ "e": "shot", "i": 1, "name": "step-2", "format": "jpeg", "data": STANDARD.encode("jpg") }),
          json!({ "e": "step", "i": 1, "do": "shot", "detail": "took a screenshot", "url": "https://a" }),
          json!({ "e": "dialog", "i": 2, "type": "alert", "message": "hi hunter22" }),
          json!({ "e": "step", "i": 2, "do": "read", "detail": "read a value", "value": { "v": "hunter22" }, "url": "https://b" }),
          json!({ "e": "done", "target": "T1", "url": "https://b", "title": "B" }),
        ],
        exited(0, ""),
      ),
    ],
    asked: RefCell::new(vec![]),
  };
  let mut kept = Kept::default();
  let result =
    drive_plan(&plan, &secrets(&[("PW", "hunter22")]), &helper(), &Bounds::default(), &node, &mut kept)
      .await;
  let asked = node.asked.borrow();

  assert_eq!(
    asked.iter().map(|env| env.contains_key("BMO_RB_HELPER")).collect::<Vec<_>>(),
    [false, true, false]
  );
  assert!(asked.iter().all(|env| env["BMO_RB_SECRET_PW"] == "hunter22"));
  assert_eq!(
    asked
      .iter()
      .map(|env| {
        let sent: Value = serde_json::from_str(&env["BMO_RB_PLAN"]).unwrap();

        (sent["start"].clone(), sent["target"].clone())
      })
      .collect::<Vec<_>>(),
    [(json!(0), Value::Null), (json!(0), Value::Null), (json!(1), json!("T1"))]
  );
  assert_eq!(kept.shots, ["1:step-2:jpeg:jpg"]);
  assert_eq!(result.failure, None);

  let text = result_text(&result, "agent-a", 3);

  assert!(!text.contains("hunter22"), "{text}");
  assert!(text.contains("1. type — typed ‹PW› into #pw (at https://a)"));
  assert!(text.contains(r#"   value: {"v":"‹PW›"}"#));
  assert!(text.contains("- step 3, alert: hi ‹PW›"));
  assert!(text.contains("- step 2, step-2: /shots/step-2.jpg"));
  assert!(text.contains("Ended on: https://b — B"));
}

// Carried from packages/web/remote-browser.test.ts:
// ends on the step that failed, and on a command that never finished
#[tokio::test]
async fn ends_on_the_step_that_failed_and_on_a_command_that_never_finished() {
  let plan = plan(&[json!({ "do": "quit" }), json!({ "do": "quit" }), json!({ "do": "quit" })]);
  let failing = Answers {
    answers: vec![(
      vec![
        json!({ "e": "fail", "i": 1, "reason": "nothing visible matches #go", "url": "https://a", "title": "A" }),
      ],
      exited(0, ""),
    )],
    asked: RefCell::new(vec![]),
  };
  let none = BTreeMap::new();
  let failed =
    drive_plan(&plan, &none, &helper(), &Bounds::default(), &failing, &mut Kept::default()).await;

  assert_eq!(
    failed.failure,
    Some(Failure {
      index: Some(1),
      reason: "nothing visible matches #go".into(),
      url: Some("https://a".into()),
      title: Some("A".into()),
    })
  );
  assert!(result_text(&failed, "n", 3).contains("Outcome: failed at step 2 of 3"));

  let killed = Answers {
    answers: vec![(vec![], exited(137, "killed\n"))],
    asked: RefCell::new(vec![]),
  };
  let cut = drive_plan(&plan, &none, &helper(), &Bounds::default(), &killed, &mut Kept::default()).await;

  assert_eq!(
    cut.failure.map(|failure| failure.reason).as_deref(),
    Some("the helper ended with 137 before it finished: killed")
  );
}

#[test]
fn a_screenshot_is_named_by_its_place_and_the_safe_part_of_its_name() {
  assert_eq!(shot_file(0, "step 1/café?", "png"), "01-step-1-caf-.png");
  assert_eq!(shot_file(11, "", "jpeg"), "12-shot.jpg");
  assert_eq!(shot_file(2, &"a".repeat(80), "png"), format!("03-{}.png", "a".repeat(60)));
}

// The node's shell stood in for by this machine's own: the command a node runs is run here, with the
// environment the exec was handed over a home of the test's own.
struct Shell {
  home: PathBuf,
  printed: RefCell<String>,
}

impl Exec for Shell {
  async fn exec(
    &self,
    command: &str,
    env: &BTreeMap<String, String>,
    lines: UnboundedSender<String>,
  ) -> ExecEnd {
    let done = tokio::process::Command::new("sh")
      .args(["-c", command])
      .env_clear()
      .env("PATH", std::env::var("PATH").unwrap_or_default())
      .env("HOME", &self.home)
      .envs(env)
      .stdin(std::process::Stdio::null())
      .output()
      .await
      .unwrap();

    self.printed.borrow_mut().push_str(&String::from_utf8_lossy(&done.stdout));

    for line in String::from_utf8_lossy(&done.stdout).lines().filter(|line| !line.trim().is_empty()) {
      lines.send(line.to_owned()).unwrap();
    }

    ExecEnd::Exit {
      code: done.status.code().map(i64::from),
      stderr: String::from_utf8_lossy(&done.stderr).into_owned(),
    }
  }
}

fn page(path: &str, body: &str) -> axum::response::Html<String> {
  axum::response::Html(format!(
    "<!doctype html><html><head><title>{path}</title></head><body>{body}</body></html>"
  ))
}

// Carried from packages/web/remote-browser.test.ts:
// types a secret it never prints, clicks inside a frame as a real mouse, reads, photographs and shuts the browser down
#[tokio::test]
async fn the_helper_drives_a_real_browser_and_never_prints_a_secret() {
  if !["google-chrome", "chromium", "microsoft-edge"].iter().any(|name| on_path(name)) {
    eprintln!("no browser on this machine; the helper's drive is not tried");
    return;
  }

  let home = tempfile::tempdir().unwrap();
  // Smooth scrolling and a long page, so a click measured before the scroll has come to rest would land
  // where the button no longer is.
  let first = r#"<style>html { scroll-behavior: smooth }</style><input id="user"><input id="pw" type="password"><div style="height: 3000px"></div>
    <button id="go" onclick="document.getElementById('out').textContent = document.getElementById('user').value + ':' + document.getElementById('pw').value.length; alert('hello ' + document.getElementById('user').value)">Go</button>
    <div id="out"></div><iframe name="inner" src="/inner" width="300" height="80"></iframe>"#;
  let app = axum::Router::new()
    .route("/", axum::routing::get(move || async move { page("/", first) }))
    .route(
      "/inner",
      axum::routing::get(|| async {
        page(
          "/inner",
          r#"<button id="press" onclick="this.textContent='pressed:' + event.isTrusted">Press me</button>"#,
        )
      }),
    )
    .route("/next", axum::routing::get(|| async { page("/next", "<p>next page</p>") }));
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let base = format!("http://{}", listener.local_addr().unwrap());
  let server = tokio::spawn(async move { axum::serve(listener, app).await });
  let port = 20_000 + i64::from(rand::random::<u16>() % 20_000);
  let plan = Plan {
    port,
    ..plan(&[
      json!({ "do": "page", "new": format!("{base}/") }),
      json!({ "do": "type", "selector": "#user", "text": "hana" }),
      json!({ "do": "type", "selector": "#pw", "secret": "PW" }),
      json!({ "do": "click", "text": "Go" }),
      json!({ "do": "read", "js": "document.getElementById('out').textContent", "as": "out" }),
      json!({ "do": "click", "text": "Press me", "frame": "inner" }),
      json!({ "do": "read", "js": "document.getElementById('press').textContent", "frame": "inner", "as": "pressed" }),
      json!({ "do": "goto", "url": format!("{base}/next") }),
      json!({ "do": "wait", "text": "next page" }),
      json!({ "do": "shot", "as": "next" }),
      json!({ "do": "quit" }),
    ])
  };
  let node = Shell {
    home: home.path().to_path_buf(),
    printed: RefCell::new(String::new()),
  };
  let mut kept = Kept::default();
  // A budget this short pauses after every step, so each step is also a resumed exec.
  let bounds = Bounds {
    budget_ms: 1,
    cap: RUN_CAP,
  };
  let result = tokio::time::timeout(
    Duration::from_secs(60),
    drive_plan(
      &plan,
      &secrets(&[("PW", "s3cret-pw")]),
      &helper_source().unwrap(),
      &bounds,
      &node,
      &mut kept,
    ),
  )
  .await;

  server.abort();

  // The browser was shut by the plan's last step; one the run left behind goes here.
  let marker = home.path().join(".local/share/bmo-remote-browser");
  let _ = std::process::Command::new("pkill").args(["-9", "-f"]).arg(&marker).status();

  let result = result.expect("the drive did not end within a minute");
  let value =
    |index: i64| result.steps.iter().find(|step| step.index == index).and_then(|step| step.value.clone());

  assert_eq!(result.failure, None);
  assert_eq!(result.browser.as_ref().map(|(launched, _)| *launched), Some(true));
  assert_eq!(value(4).as_deref(), Some(r#""hana:9""#));
  assert_eq!(value(6).as_deref(), Some(r#""pressed:true""#));
  assert_eq!(
    result.dialogs,
    [Dialog {
      index: 3,
      kind: "alert".into(),
      message: "hello hana".into(),
    }]
  );
  assert_eq!(result.ended_on.as_ref().and_then(|(url, _)| url.clone()), None);
  assert_eq!(kept.shots.len(), 1);
  assert!(kept.shots[0].len() > 500);
  assert!(!node.printed.borrow().contains("s3cret-pw"));
  assert!(kept.notes.iter().all(|note| !note.contains("s3cret-pw")));
  assert_eq!(result.steps.len(), plan.steps.len());
}
