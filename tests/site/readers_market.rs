use bmo_explorer::site::readers::market::*;

fn rows() -> Vec<MarketRow> {
  vec![
    MarketRow {
      date: "2026-09-28".into(),
      value: 271_500.0,
      change: -14_000.0,
      ratio: -4.9,
      ..MarketRow::default()
    },
    MarketRow {
      date: "2026-09-23".into(),
      value: 286_500.0,
      change: 10_000.0,
      ratio: 3.62,
      ..MarketRow::default()
    },
  ]
}

fn quote(traded_at: &str, status: &str) -> MarketQuote {
  MarketQuote {
    value: 271_500.0,
    change: -14_000.0,
    ratio: -4.9,
    traded_at: traded_at.into(),
    status: status.into(),
  }
}

fn states(days: &[MarketDay]) -> Vec<DayState> {
  days.iter().map(|day| day.state).collect()
}

// Carried from packages/web/market-session.test.ts: the row of a session still running is marked in session, the ones before it are closes
#[test]
fn the_row_of_a_running_session_is_marked_in_session() {
  assert_eq!(
    states(&session_days(&rows(), Some(&quote("2026-09-28T12:40:19+09:00", "OPEN")))),
    [DayState::Session, DayState::Close]
  );
}

// Carried from packages/web/market-session.test.ts: once the market has closed, the day's row is its close
#[test]
fn once_the_market_has_closed_the_days_row_is_its_close() {
  assert_eq!(
    states(&session_days(&rows(), Some(&quote("2026-09-28T15:30:00+09:00", "CLOSE")))),
    [DayState::Close, DayState::Close]
  );
}

// Carried from packages/web/market-session.test.ts: a quote newer than every row stands as the newest row, in session or closed as it says
#[test]
fn a_quote_newer_than_every_row_stands_as_the_newest_row() {
  let listed = &rows()[1..];
  let open = session_days(listed, Some(&quote("2026-09-28T12:36:01+09:00", "OPEN")));

  assert_eq!(
    (open[0].row.date.as_str(), open[0].row.value, open[0].state),
    ("2026-09-28", 271_500.0, DayState::Session)
  );

  let closed = session_days(listed, Some(&quote("2026-09-28T16:10:00+09:00", "CLOSE")));

  assert_eq!(
    closed.iter().map(|day| (day.row.date.as_str(), day.state)).collect::<Vec<_>>(),
    [("2026-09-28", DayState::Close), ("2026-09-23", DayState::Close)]
  );
}

// Carried from packages/web/market-session.test.ts: without a quote the newest row is left unconfirmed rather than given as a close
#[test]
fn without_a_quote_the_newest_row_is_left_unconfirmed() {
  assert_eq!(states(&session_days(&rows(), None)), [DayState::Unconfirmed, DayState::Close]);
}

// Carried from packages/web/market-session.test.ts: is given in KST, with a foreign market's own clock beside it
#[test]
fn the_time_is_given_in_kst_with_a_foreign_markets_own_clock_beside_it() {
  assert_eq!(as_of_text("2026-09-28T12:40:19+09:00", "Seoul"), "2026-09-28 12:40 KST");
  assert_eq!(
    as_of_text("2026-09-27T23:37:39-04:00", "New York"),
    "2026-09-28 12:37 KST (New York 2026-09-27 23:37)"
  );
}
