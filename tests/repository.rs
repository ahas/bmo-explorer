//! Checks that span the repository: what its packages may depend on, where its tests stand, how its folders are
//! named, and what no tracked file may hold. Each check is a function over what it reads, held both to the
//! repository and to a made-up case of what it refuses, so a check that has gone blind fails too.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::net::Ipv4Addr;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde_json::{Value, json};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// The only packages from another repository a package here may depend on: the base libraries.
const BASE_LIBRARIES: [&str; 4] = ["bmo-chat", "bmo-chat-client", "bmo-ui", "bmo-markup"];

/// The dependency tables of a `package.json`.
const PACKAGE_TABLES: [&str; 4] =
  ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"];

/// The ledger of the lines holding Korean each code file may have.
const LEDGER: &str = "korean-ledger.json";

/// The extensions of the code and test files held to the ledger. Recorded input under a `fixtures/` folder holds
/// words somebody else wrote and is not.
const CODE_EXTENSIONS: [&str; 7] = ["rs", "ts", "js", "mjs", "py", "sh", "toml"];

/// The Hangul blocks of Unicode: Jamo, compatibility Jamo, Jamo extended A and B, syllables, and the halfwidth
/// forms.
const HANGUL: [(u32, u32); 6] = [
  (0x1100, 0x11FF),
  (0x3130, 0x318F),
  (0xA960, 0xA97F),
  (0xAC00, 0xD7AF),
  (0xD7B0, 0xD7FF),
  (0xFFA0, 0xFFDC),
];

/// The variable naming the file of names no tracked file may hold: agents', people's and machines'. No
/// repository keeps such a list, so it is read from outside the checkout.
const NAMES_LIST: &str = "BMO_NAMES_LIST";

/// Path prefixes a home directory is written under.
const HOME_PREFIXES: [&str; 2] = ["/home/", "/Users/"];

/// The manifests of another language that may not stand at the root beside the Cargo manifest.
const OTHER_MANIFESTS: [&str; 5] = ["package.json", "pyproject.toml", "bun.lock", "go.mod", "deno.json"];

/// The tables of the testbed's declaration format.
const TESTBED_TABLES: [&str; 8] =
  ["test", "projects", "tools", "check", "build", "deploy", "runner", "events"];

fn root() -> PathBuf {
  PathBuf::from(ROOT)
}

fn metadata() -> Value {
  let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
  let output = Command::new(cargo)
    .args(["metadata", "--no-deps", "--format-version", "1"])
    .current_dir(ROOT)
    .output()
    .expect("cargo metadata runs");

  assert!(
    output.status.success(),
    "cargo metadata failed: {}",
    String::from_utf8_lossy(&output.stderr)
  );
  serde_json::from_slice(&output.stdout).expect("cargo metadata answers JSON")
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

/// A path with its `.` and `..` parts taken away, read without the file system, since a path a manifest names
/// need not exist.
fn lexical(path: &Path) -> PathBuf {
  let mut clean = PathBuf::new();

  for part in path.components() {
    match part {
      Component::CurDir => {}
      Component::ParentDir => {
        clean.pop();
      }
      other => clean.push(other),
    }
  }

  clean
}

fn is_inside(root: &Path, path: &Path) -> bool {
  lexical(path).starts_with(lexical(root))
}

fn is_base_library(name: &str) -> bool {
  BASE_LIBRARIES.contains(&name)
}

/// Whether a package's name says it is one of this product's units.
fn is_unit_name(name: &str) -> bool {
  name.starts_with("bmo-")
    || name.starts_with("bmo_")
    || name.starts_with("@bmo/")
    || name.starts_with("@bmo-")
}

/// The crate dependencies `cargo metadata` lists that the repository may not hold, each as
/// `<manifest>: <crate> (<why>)`: a path outside the repository, a git source, or another unit's crate, unless
/// the crate is a base library.
fn refused_crates(metadata: &Value, root: &Path) -> Vec<String> {
  let mut refused = Vec::new();

  for package in metadata["packages"].as_array().into_iter().flatten() {
    let manifest = package["manifest_path"].as_str().unwrap_or("a manifest");

    for dependency in package["dependencies"].as_array().into_iter().flatten() {
      let name = dependency["name"].as_str().unwrap_or_default();

      if is_base_library(name) {
        continue;
      }

      let why = match (dependency["path"].as_str(), dependency["source"].as_str()) {
        (Some(path), _) if is_inside(root, Path::new(path)) => None,
        (Some(_), _) => Some("a path outside the repository"),
        (None, Some(source)) if source.starts_with("git+") => Some("a git source"),
        _ if is_unit_name(name) => Some("another unit's crate"),
        _ => None,
      };

      if let Some(why) = why {
        refused.push(format!("{manifest}: {name} ({why})"));
      }
    }
  }

  refused
}

/// The dependencies a `package.json` names that the repository may not hold, read as `refused_crates` reads a
/// crate's: a file or link path outside the repository, a git or web source, or another unit's package.
fn refused_packages(manifest: &Path, package: &Value, root: &Path) -> Vec<String> {
  let folder = manifest.parent().unwrap_or(root);
  let mut refused = Vec::new();

  for table in PACKAGE_TABLES {
    for (name, spec) in package[table].as_object().into_iter().flatten() {
      if is_base_library(name) {
        continue;
      }

      let spec = spec.as_str().unwrap_or_default();
      let local = ["file:", "link:", "portal:"].iter().find_map(|scheme| spec.strip_prefix(scheme));
      let why = match local {
        Some(path) if is_inside(root, &folder.join(path)) => None,
        Some(_) => Some("a path outside the repository"),
        None if spec.starts_with("workspace:") => None,
        None if spec.contains("://") || spec.starts_with("git") || spec.starts_with("github:") => {
          Some("a git or web source")
        }
        None if is_unit_name(name) => Some("another unit's package"),
        None => None,
      };

      if let Some(why) = why {
        refused.push(format!("{}: {name} ({why})", manifest.display()));
      }
    }
  }

  refused
}

fn is_hangul(letter: char) -> bool {
  let point = u32::from(letter);

  HANGUL.iter().any(|&(first, last)| (first..=last).contains(&point))
}

/// How many lines of a text hold Hangul.
fn hangul_lines(text: &str) -> usize {
  text.lines().filter(|line| line.chars().any(is_hangul)).count()
}

/// Whether a path is a code or test file the ledger holds: one of the code extensions, outside recorded input.
fn is_code_file(path: &str) -> bool {
  let recorded = Path::new(path).components().any(|part| part == Component::Normal("fixtures".as_ref()));
  let extension = Path::new(path).extension().and_then(|extension| extension.to_str());

  !recorded && extension.is_some_and(|extension| CODE_EXTENSIONS.contains(&extension))
}

fn ledger() -> BTreeMap<String, usize> {
  let text = text_of(LEDGER).expect("the ledger is at the root");

  serde_json::from_str(&text).expect("the ledger is a JSON object from paths to counts")
}

/// The code files holding more lines with Korean than the ledger allows them, each with both counts.
fn past_the_ledger(texts: &[(String, String)], ledger: &BTreeMap<String, usize>) -> Vec<String> {
  texts
    .iter()
    .filter(|(path, _)| is_code_file(path))
    .filter_map(|(path, text)| {
      let held = hangul_lines(text);
      let allowed = ledger.get(path).copied().unwrap_or(0);

      (held > allowed).then(|| format!("{path}: {held} lines with Korean, the ledger allows {allowed}"))
    })
    .collect()
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

/// An address in a range only a private network uses: the private ranges, and the shared range carrier-grade
/// and overlay networks hand out.
fn is_private_network(address: Ipv4Addr) -> bool {
  let [first, second, ..] = address.octets();

  address.is_private() || (first == 100 && second & 0b1100_0000 == 0b0100_0000)
}

fn private_addresses(text: &str) -> Vec<Ipv4Addr> {
  text
    .split(|letter: char| !letter.is_ascii_digit() && letter != '.')
    .filter_map(|run| run.trim_matches('.').parse::<Ipv4Addr>().ok())
    .filter(|&address| is_private_network(address))
    .collect()
}

fn holds_home_path(text: &str) -> bool {
  HOME_PREFIXES.iter().any(|prefix| {
    text.match_indices(prefix).any(|(start, found)| {
      text[start + found.len()..]
        .chars()
        .next()
        .is_some_and(|next| is_word_character(next) || next == '.')
    })
  })
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

/// The first cell of every body row of the README's table whose first header cell is `Folder`, without the
/// code span's marks and the trailing slash.
fn readme_folders(readme: &str) -> BTreeSet<String> {
  let mut folders = BTreeSet::new();
  let mut in_head = false;
  let mut cell = 0usize;
  let mut text = String::new();
  let mut is_folder_table = false;
  let mut header_first = None::<String>;

  for event in Parser::new_ext(readme, Options::ENABLE_TABLES) {
    match event {
      Event::Start(Tag::Table(_)) => {
        is_folder_table = false;
        header_first = None;
      }
      Event::Start(Tag::TableHead) => {
        in_head = true;
        cell = 0;
      }
      Event::End(TagEnd::TableHead) => {
        in_head = false;
        is_folder_table = header_first.as_deref() == Some("Folder");
      }
      Event::Start(Tag::TableRow) => cell = 0,
      Event::Start(Tag::TableCell) => text.clear(),
      Event::End(TagEnd::TableCell) => {
        if in_head && cell == 0 {
          header_first = Some(text.trim().to_owned());
        } else if !in_head && is_folder_table && cell == 0 {
          folders.insert(text.trim().trim_end_matches('/').to_owned());
        }
        cell += 1;
      }
      Event::Text(piece) | Event::Code(piece) => text.push_str(&piece),
      _ => {}
    }
  }

  folders
}

#[test]
fn every_dependency_is_from_a_registry_inside_the_repository_or_a_base_library() {
  let root = root();
  let mut refused = refused_crates(&metadata(), &root);

  for path in tracked_files().into_iter().filter(|path| file_name(path) == "package.json") {
    let manifest = root.join(&path);
    let package: Value = serde_json::from_str(&text_of(&path).expect("a manifest is text"))
      .unwrap_or_else(|why| panic!("{path} does not read as JSON: {why}"));

    refused.extend(refused_packages(&manifest, &package, &root));
  }

  assert!(refused.is_empty(), "dependencies the repository may not hold:\n{}", refused.join("\n"));
}

#[test]
fn the_dependency_check_refuses_another_units_package_a_path_outside_and_a_git_source() {
  let root = Path::new("/work/explorer");
  let metadata = json!({ "packages": [{
    "manifest_path": "/work/explorer/tools/web_fetch/Cargo.toml",
    "dependencies": [
      { "name": "bmo-explorer", "source": null, "path": "/work/explorer" },
      { "name": "bmo-markup", "source": null, "path": "/work/markup" },
      { "name": "serde", "source": "registry+https://github.com/rust-lang/crates.io-index", "path": null },
      { "name": "bmo-tool", "source": null, "path": "/work/explorer/../pocket/sdk/contract" },
      { "name": "helper", "source": "git+https://example.com/helper", "path": null },
      { "name": "bmo-agency", "source": "registry+https://github.com/rust-lang/crates.io-index", "path": null },
    ],
  }]});

  assert_eq!(
    refused_crates(&metadata, root),
    [
      "/work/explorer/tools/web_fetch/Cargo.toml: bmo-tool (a path outside the repository)",
      "/work/explorer/tools/web_fetch/Cargo.toml: helper (a git source)",
      "/work/explorer/tools/web_fetch/Cargo.toml: bmo-agency (another unit's crate)",
    ]
  );

  let manifest = root.join("tests/site-placed/package.json");
  let package = json!({
    "dependencies": { "bmo-ui": "file:../../../ui", "left-pad": "1.3.0" },
    "devDependencies": { "@bmo/bot": "1.0.0", "shared": "file:../../../shared", "near": "file:../fixtures" },
    "optionalDependencies": { "remote": "github:example/remote" },
  });

  assert_eq!(
    refused_packages(&manifest, &package, root),
    [
      "/work/explorer/tests/site-placed/package.json: @bmo/bot (another unit's package)",
      "/work/explorer/tests/site-placed/package.json: shared (a path outside the repository)",
      "/work/explorer/tests/site-placed/package.json: remote (a git or web source)",
    ]
  );
}

#[test]
fn the_version_file_holds_the_workspaces_version() {
  let version = text_of("VERSION").expect("VERSION is at the root");

  assert_eq!(version.trim(), env!("CARGO_PKG_VERSION"));
  assert_eq!(bmo_explorer::VERSION, version.trim());
}

#[test]
fn the_testbed_declaration_is_in_the_testbeds_format() {
  let declaration: toml::Table = text_of("testbed.toml")
    .expect("testbed.toml is at the root")
    .parse()
    .expect("testbed.toml is TOML");
  let strange: Vec<&String> = declaration
    .keys()
    .filter(|key| !["testbed", "name"].contains(&key.as_str()) && !TESTBED_TABLES.contains(&key.as_str()))
    .collect();

  assert_eq!(declaration.get("testbed").and_then(toml::Value::as_integer), Some(0));
  assert!(strange.is_empty(), "testbed.toml holds keys its format does not name: {strange:?}");
  for key in TESTBED_TABLES.iter().filter(|key| declaration.contains_key(**key)) {
    assert!(declaration[*key].is_table(), "`{key}` in testbed.toml is a table");
  }
  assert_eq!(
    declaration["check"].get("base_libraries").and_then(toml::Value::as_array),
    Some(&BASE_LIBRARIES.iter().map(|&name| toml::Value::from(name)).collect())
  );
  assert_eq!(declaration["check"].get("korean_ledger").and_then(toml::Value::as_str), Some(LEDGER));
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
fn the_readme_names_every_top_level_folder_and_no_other() {
  let standing: BTreeSet<String> = tracked_files()
    .into_iter()
    .filter_map(|path| path.split_once('/').map(|(folder, _)| folder.to_owned()))
    .collect();
  let named = readme_folders(&text_of("README.md").expect("README.md is at the root"));

  assert_eq!(named, standing, "the README's folder table and the top-level folders differ");
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

#[test]
fn no_code_file_holds_more_korean_than_the_ledger_allows() {
  let found = past_the_ledger(&tracked_texts(), &ledger());

  assert!(found.is_empty(), "these files hold Korean past the ledger:\n{}", found.join("\n"));
}

#[test]
fn the_ledger_check_counts_korean_lines_in_code_and_passes_recorded_input() {
  let texts = [
    (
      "src/search/engines.rs",
      "let query = \"날씨\";\nlet other = \"weather\";\nlet again = \"서울 날씨\";",
    ),
    ("src/search/naver.rs", "let page = \"검색\";"),
    ("tests/fixtures/engines/naver-web.html", "<p>검색 결과</p>"),
    ("docs/web-access.md", "Naver's 접속 제한 page"),
    ("src/web/client.rs", "caf\u{e9} and \u{4e2d}"),
  ]
  .map(|(path, text)| (path.to_owned(), text.to_owned()));
  let ledger = BTreeMap::from([("src/search/engines.rs".to_owned(), 1)]);

  assert_eq!(
    past_the_ledger(&texts, &ledger),
    [
      "src/search/engines.rs: 2 lines with Korean, the ledger allows 1",
      "src/search/naver.rs: 1 lines with Korean, the ledger allows 0",
    ]
  );
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

#[test]
fn no_home_paths_in_tracked_files() {
  let found: Vec<String> = tracked_texts()
    .into_iter()
    .filter(|(_, text)| holds_home_path(text))
    .map(|(path, _)| path)
    .collect();

  assert!(found.is_empty(), "home paths in tracked files: {found:?}");
}

/// The tests of the address checks refuse private addresses by name, so the files under `tests/` are their input
/// and are not held to this.
#[test]
fn no_private_network_addresses_outside_the_tests() {
  let found: Vec<String> = tracked_texts()
    .into_iter()
    .filter(|(path, _)| !path.starts_with("tests/"))
    .flat_map(|(path, text)| {
      private_addresses(&text).into_iter().map(move |address| format!("{path}: {address}"))
    })
    .collect();

  assert!(found.is_empty(), "private network addresses in tracked files: {found:?}");
}

#[test]
fn a_home_path_and_a_private_address_are_found() {
  // Made from the prefixes, since this file is itself held to the check.
  assert!(holds_home_path(&format!("the profile under {}someone/.config", HOME_PREFIXES[0])));
  assert!(holds_home_path(&format!("on a Mac at {}someone", HOME_PREFIXES[1])));
  assert!(!holds_home_path("a page at https://example.com/home/ and /home/"));
  assert_eq!(
    private_addresses("10.1.2.3, 192.168.0.9, 100.64.0.1, 8.8.8.8 and 172.16.5.4."),
    ["10.1.2.3", "192.168.0.9", "100.64.0.1", "172.16.5.4"]
      .map(|address| address.parse::<Ipv4Addr>().unwrap())
  );
}
