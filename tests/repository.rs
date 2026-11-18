//! Checks that span the repository: what its packages may depend on, where its tests stand, how its folders are
//! named, and what no tracked file may hold. Each check is a function over what it reads, held both to the
//! repository and to a made-up case of what it refuses, so a check that has gone blind fails too.

use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// The variable naming the file of names no tracked file may hold: agents', people's and machines'. No
/// repository keeps such a list, so it is read from outside the checkout.
const NAMES_LIST: &str = "BMO_NAMES_LIST";

/// The manifests of another language that may not stand at the root beside the Cargo manifest.
const OTHER_MANIFESTS: [&str; 5] = ["package.json", "pyproject.toml", "bun.lock", "go.mod", "deno.json"];

fn root() -> PathBuf {
  PathBuf::from(ROOT)
}

/// Every file git tracks or would track: the committed ones and the new ones it does not ignore.
fn tracked_files() -> Vec<String> {
  let output = Command::new("git")
    .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
    .current_dir(ROOT)
    .output()
    .expect("git ls-files runs");

  assert!(
    output.status.success(),
    "git ls-files failed: {}",
    String::from_utf8_lossy(&output.stderr)
  );

  let mut files: Vec<String> = String::from_utf8(output.stdout)
    .expect("tracked paths are UTF-8")
    .split('\0')
    .filter(|path| !path.is_empty() && Path::new(ROOT).join(path).is_file())
    .map(str::to_owned)
    .collect();

  files.sort();
  files.dedup();
  files
}

/// The text of a tracked file, or nothing when it is gone from the work tree or is not UTF-8 text.
fn text_of(path: &str) -> Option<String> {
  match fs::read(Path::new(ROOT).join(path)) {
    Ok(bytes) => String::from_utf8(bytes).ok(),
    Err(error) if error.kind() == ErrorKind::NotFound => None,
    Err(error) => panic!("{path} is unreadable: {error}"),
  }
}

fn tracked_texts() -> Vec<(String, String)> {
  tracked_files()
    .into_iter()
    .filter_map(|path| text_of(&path).map(|text| (path, text)))
    .collect()
}

fn file_name(path: &str) -> &str {
  Path::new(path).file_name().and_then(|name| name.to_str()).unwrap_or(path)
}

fn is_word_character(letter: char) -> bool {
  letter.is_alphanumeric() || letter == '_'
}

/// Whether `word` stands in `text` as a whole word, not as part of a longer one.
fn holds_word(text: &str, word: &str) -> bool {
  text.match_indices(word).any(|(start, found)| {
    let before = text[..start].chars().next_back();
    let after = text[start + found.len()..].chars().next();

    !before.is_some_and(is_word_character) && !after.is_some_and(is_word_character)
  })
}

fn names_list(names: &toml::Table, from: &str, key: &str) -> Vec<String> {
  names
    .get(key)
    .and_then(toml::Value::as_array)
    .map(|listed| {
      listed
        .iter()
        .map(|name| name.as_str().unwrap_or_else(|| panic!("{from}: `{key}` lists strings")).to_owned())
        .collect()
    })
    .unwrap_or_default()
}

/// The names a text holds, as the names list reads them: `words` as whole words ignoring case, `exact_words` as
/// whole words with their case kept, `anywhere` anywhere, after the `allowed` terms are set aside.
fn listed_names(text: &str, names: &toml::Table, from: &str) -> Vec<String> {
  let allowed = names_list(names, from, "allowed");
  let text = allowed.iter().fold(text.to_owned(), |text, term| text.replace(term.as_str(), " "));
  let lowered = text.to_lowercase();
  let words = names_list(names, from, "words").into_iter().map(|name| name.to_lowercase());
  let exact_words = names_list(names, from, "exact_words");
  let anywhere = names_list(names, from, "anywhere");

  words
    .filter(|name| holds_word(&lowered, name))
    .chain(exact_words.into_iter().filter(|name| holds_word(&text, name)))
    .chain(anywhere.into_iter().filter(|name| text.contains(name.as_str())))
    .collect()
}

/// Whether a file is a test by its own name: a Bun or Node test file, or a Python one.
fn is_test_file_by_name(path: &str) -> bool {
  let name = file_name(path);

  name.contains(".test.")
    || name.contains(".spec.")
    || (name.starts_with("test_") && name.ends_with(".py"))
    || name.ends_with("_test.py")
}

/// Whether Rust source holds a test or a test module of its own.
fn holds_rust_test(text: &str) -> bool {
  text
    .lines()
    .map(str::trim_start)
    .any(|line| line.starts_with("#[cfg(test)]") || line.starts_with("#[test]"))
}

/// The tracked files that are tests standing outside `tests/`.
fn tests_outside(texts: &[(String, String)]) -> Vec<String> {
  texts
    .iter()
    .filter(|(path, _)| !path.starts_with("tests/"))
    .filter(|(path, text)| is_test_file_by_name(path) || (path.ends_with(".rs") && holds_rust_test(text)))
    .map(|(path, _)| path.clone())
    .collect()
}

/// The tracked paths with a folder named after the folder holding it.
fn nested_twins(paths: &[String]) -> Vec<String> {
  paths
    .iter()
    .filter(|path| {
      let parts: Vec<Component> = Path::new(path.as_str()).components().collect();

      parts.windows(2).take(parts.len().saturating_sub(2)).any(|pair| pair[0] == pair[1])
    })
    .cloned()
    .collect()
}

#[test]
fn the_version_file_holds_the_workspaces_version() {
  let version = text_of("VERSION").expect("VERSION is at the root");

  assert_eq!(version.trim(), env!("CARGO_PKG_VERSION"));
  assert_eq!(bmo_explorer::VERSION, version.trim());
}

#[test]
fn only_the_cargo_manifest_at_the_root() {
  let found: Vec<String> = fs::read_dir(ROOT)
    .expect("the repository root is readable")
    .map(|entry| entry.expect("a root entry is readable").file_name().to_string_lossy().into_owned())
    .filter(|name| OTHER_MANIFESTS.contains(&name.as_str()))
    .collect();

  assert!(found.is_empty(), "the root holds {found:?}; only the Cargo manifest belongs there");
  assert!(
    !root().join("crates").exists(),
    "the repository is one crate and holds no crates/ folder"
  );
}

#[test]
fn no_directory_is_named_after_its_parent() {
  let twins = nested_twins(&tracked_files());

  assert!(twins.is_empty(), "these paths repeat a directory inside itself: {twins:?}");
}

#[test]
fn a_directory_named_after_its_parent_is_found() {
  let paths =
    ["web/web/lib.rs", "tools/web_fetch/tool.toml", "docs/docs.md", "src/site/site.rs"].map(String::from);

  assert_eq!(nested_twins(&paths), ["web/web/lib.rs"]);
}

#[test]
fn tests_stand_only_in_tests() {
  let outside = tests_outside(&tracked_texts());

  assert!(outside.is_empty(), "tests outside tests/: {outside:?}");
}

#[test]
fn a_test_is_told_by_its_name_and_its_attributes() {
  let texts = [
    ("tests/web.rs", "#[test]\nfn reads() {}"),
    ("src/web/pacing.rs", "fn a() {}\n  #[cfg(test)]\nmod tests {}"),
    ("src/web/clock.rs", "// a test of the clock\nfn a() {}"),
    ("tools/web_fetch/page.test.ts", "test('reads', () => {})"),
  ]
  .map(|(path, text)| (path.to_owned(), text.to_owned()));

  assert_eq!(tests_outside(&texts), ["src/web/pacing.rs", "tools/web_fetch/page.test.ts"]);
}

/// The names list, as `BMO_NAMES_LIST` names it. Where no list is named, the names are not checked here and the
/// run says so.
#[test]
fn no_listed_names_in_tracked_files() {
  let Some(from) = std::env::var_os(NAMES_LIST) else {
    eprintln!("{NAMES_LIST} names no list; the names were not checked");
    return;
  };
  let from = from.to_string_lossy().into_owned();
  let names: toml::Table = fs::read_to_string(&from)
    .unwrap_or_else(|error| panic!("{from} is unreadable: {error}"))
    .parse()
    .unwrap_or_else(|error| panic!("{from} is not TOML: {error}"));
  let found: Vec<String> = tracked_texts()
    .into_iter()
    .flat_map(|(path, text)| {
      listed_names(&text, &names, &from)
        .into_iter()
        .map(move |name| format!("{path}: {name}"))
    })
    .collect();

  assert!(found.is_empty(), "listed names in tracked files: {found:?}");
}

#[test]
fn a_listed_name_is_found_as_a_word_in_its_case_and_anywhere() {
  let names: toml::Table = r#"
    words = ["Example-Host"]
    exact_words = ["Agent"]
    anywhere = ["neutral.example"]
    allowed = ["Agentic"]
  "#
  .parse()
  .unwrap();
  let text = "runs on example-host as Agent; an agent and Agentic tools; mail@neutral.example.org";

  assert_eq!(listed_names(text, &names, "a list"), ["example-host", "Agent", "neutral.example"]);
  assert!(listed_names("example-hosts and agent", &names, "a list").is_empty());
}
