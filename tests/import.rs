//! The old archive's four web-reading tables moved into the store (TODO #1923): the mapping, the refusal of
//! a store already holding rows, the dry run, and the count-and-sample compare.

use std::path::Path;

use bmo_explorer::import;
use bmo_explorer::store;

fn export(folder: &Path, counts: [(u64, &str); 4]) {
  let files = ["web_engine_cookie", "web_engine_health", "web_engine_recipe", "web_host_pacing"];
  let mut tables = Vec::new();
  for (index, (count, key)) in counts.iter().enumerate() {
    tables.push(format!(
      r#"{{"source":"{}","file":"{}.ndjson","count":{count},"key":[{key}]}}"#,
      files[index], files[index]
    ));
  }
  let said = format!(
    r#"{{"snapshot":"1:2:","readAt":"2026-10-21T17:00:00+09:00","tables":[{}]}}"#,
    tables.join(",")
  );
  std::fs::write(folder.join("manifest.json"), said).expect("the manifest is written");
  for file in files {
    let lines = if file == "web_engine_cookie" {
      vec![
        r#"{"engine":"edge","host":"example.com","path":"/","name":"a","value":"b","secure":0,"expires_at":1791866256631,"rowid":1}"#,
        r#"{"engine":"edge","host":"example.com","path":"/","name":"c","value":"d","secure":1,"expires_at":1791866256632,"rowid":2}"#,
      ]
    } else if file == "web_engine_health" {
      vec![
        r#"{"engine":"edge","state":"ready","reason":null,"observed_at":1791866256631,"suspended_until":null,"elapsed_ms":12,"result_count":3,"rowid":1}"#,
      ]
    } else if file == "web_engine_recipe" {
      vec![
        r#"{"engine":"edge","identity":"one","interval_ms":1000,"route":"search","warm_up":1,"cause":null,"evidence":null,"set_at":1791866256631,"rowid":1}"#,
      ]
    } else {
      vec![r#"{"key":"example.com","next_at":1791866256631,"updated_at":1791866256631,"rowid":1}"#]
    };
    std::fs::write(folder.join(format!("{file}.ndjson")), lines.join("\n") + "\n")
      .expect("the rows are written");
  }
}

fn counted(database: &Path) -> u64 {
  let runtime = tokio::runtime::Runtime::new().expect("the runtime starts");
  runtime.block_on(async {
    let mut connection = store::open(database).await.expect("the store opens");
    let mut total = 0;
    for table in import::TABLES {
      let held: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM \"{}\"", table.target))
        .fetch_one(&mut connection)
        .await
        .expect("the table is counted");
      total += held as u64;
    }
    total
  })
}

#[test]
fn the_four_tables_are_moved_and_compared() {
  let folder = tempfile::tempdir().expect("a scratch folder");
  export(
    folder.path(),
    [
      (2, "\"engine\",\"host\",\"path\",\"name\""),
      (1, "\"engine\""),
      (1, "\"engine\""),
      (1, "\"key\""),
    ],
  );

  let database = folder.path().join("explorer.db");
  let report = tokio::runtime::Runtime::new()
    .expect("the runtime starts")
    .block_on(import::run(&database, folder.path(), false))
    .expect("the import runs");

  assert_eq!(report.tables.iter().map(|table| table.written).collect::<Vec<_>>(), [2, 1, 1, 1]);
  assert_eq!(report.sample.checked, 5);
  assert_eq!(report.sample.failures, Vec::<String>::new());
  assert_eq!(counted(&database), 5);
}

#[test]
fn a_dry_run_writes_nothing() {
  let folder = tempfile::tempdir().expect("a scratch folder");
  export(
    folder.path(),
    [
      (2, "\"engine\",\"host\",\"path\",\"name\""),
      (1, "\"engine\""),
      (1, "\"engine\""),
      (1, "\"key\""),
    ],
  );

  let database = folder.path().join("explorer.db");
  tokio::runtime::Runtime::new()
    .expect("the runtime starts")
    .block_on(import::run(&database, folder.path(), true))
    .expect("the dry run runs");
  assert_eq!(counted(&database), 0);
}

#[test]
fn a_store_holding_rows_is_refused() {
  let folder = tempfile::tempdir().expect("a scratch folder");
  export(
    folder.path(),
    [
      (2, "\"engine\",\"host\",\"path\",\"name\""),
      (1, "\"engine\""),
      (1, "\"engine\""),
      (1, "\"key\""),
    ],
  );

  let database = folder.path().join("explorer.db");
  let runtime = tokio::runtime::Runtime::new().expect("the runtime starts");
  runtime
    .block_on(import::run(&database, folder.path(), false))
    .expect("the first import runs");
  let refused = runtime
    .block_on(import::run(&database, folder.path(), false))
    .expect_err("the second is refused");
  assert!(refused.contains("already holds rows"), "{refused}");
}
