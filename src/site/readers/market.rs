//! What the Naver Securities item tools share: the item a query names, by its code or the site's own search, and
//! the addresses it is read at; a day-by-day list read beside the item's own quote, and the rule that
//! says which of its rows is a close (docs/site-contract.md, 「Where a tool lives」). A list names the
//! day being traded before that day's session has ended, and a list that names closed sessions only says
//! nothing of today, so neither is read alone: the quote says whether the market is open and when it last
//! traded, and the rows are marked by it.

use std::sync::LazyLock;

use chrono::{DateTime, FixedOffset};
use regex::Regex;
use serde_json::Value;

use crate::site::readers::url_component;

/// The item's own latest value, as its quote gave it.
#[derive(Clone, Debug, PartialEq)]
pub struct MarketQuote {
  pub value: f64,
  pub change: f64,
  pub ratio: f64,
  /// The time it was traded, with its market's own offset, as the site wrote it.
  pub traded_at: String,
  /// The market's state as the site names it: OPEN, CLOSE, or whatever else it says.
  pub status: String,
}

/// One day's value, as a list row gave it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MarketRow {
  /// The day, in its own market's calendar.
  pub date: String,
  pub value: f64,
  pub change: f64,
  pub ratio: f64,
  pub open: Option<f64>,
  pub high: Option<f64>,
  pub low: Option<f64>,
  pub volume: Option<f64>,
}

/// A settled session's close, the session still running when it was read, or a newest row whose session could
/// not be told, since the quote saying whether the market is open was not read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DayState {
  Close,
  Session,
  Unconfirmed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MarketDay {
  pub row: MarketRow,
  pub state: DayState,
}

pub fn day_label(day: &MarketDay) -> &'static str {
  match day.state {
    DayState::Close => "close",
    DayState::Session => "in session",
    DayState::Unconfirmed => "unconfirmed",
  }
}

/// A figure the site gave as a number or a text with grouping commas; NaN where it is neither.
pub fn figure(value: &Value) -> f64 {
  let text = match value {
    Value::String(text) => text.clone(),
    Value::Number(number) => number.to_string(),
    Value::Null => String::new(),
    other => other.to_string(),
  };

  // As a page's own `Number` reads it: an empty text is nought.
  let cleaned = text.replace(',', "");

  if cleaned.trim().is_empty() { 0.0 } else { cleaned.trim().parse().unwrap_or(f64::NAN) }
}

/// A figure the site gave, or none where it gave none or gave something that is not one.
pub fn optional_figure(value: &Value) -> Option<f64> {
  match value {
    Value::Null => None,
    Value::String(text) if text.is_empty() => None,
    other => Some(figure(other)).filter(|read| read.is_finite()),
  }
}

pub fn is_closed(quote: &MarketQuote) -> bool {
  quote.status == "CLOSE"
}

/// The rows newest first, the quote's own day added where no row names it yet, and every row the quote's
/// still-running session covers marked as such. Without a quote nothing can be said of the newest row's
/// session, so that row is marked unconfirmed rather than given as a close.
pub fn session_days(rows: &[MarketRow], quote: Option<&MarketQuote>) -> Vec<MarketDay> {
  let Some(quote) = quote else {
    return rows
      .iter()
      .enumerate()
      .map(|(at, row)| MarketDay {
        row: row.clone(),
        state: if at == 0 { DayState::Unconfirmed } else { DayState::Close },
      })
      .collect();
  };
  let date: String = quote.traded_at.chars().take(10).collect();
  let state = if is_closed(quote) { DayState::Close } else { DayState::Session };
  let mut days: Vec<MarketDay> = rows
    .iter()
    .map(|row| MarketDay {
      row: row.clone(),
      state: if row.date == date { state } else { DayState::Close },
    })
    .collect();

  if rows.first().is_none_or(|newest| date > newest.date) {
    days.insert(
      0,
      MarketDay {
        row: MarketRow {
          date,
          value: quote.value,
          change: quote.change,
          ratio: quote.ratio,
          ..MarketRow::default()
        },
        state,
      },
    );
  }

  days
}

pub fn state_text(quote: &MarketQuote) -> String {
  if quote.status == "OPEN" {
    "in session".into()
  } else if is_closed(quote) {
    "closed".into()
  } else {
    format!("market state {}", quote.status)
  }
}

/// The quote's time in KST, with the market's own clock beside it where the market is abroad.
pub fn as_of_text(traded_at: &str, place: &str) -> String {
  let Ok(at) = DateTime::parse_from_rfc3339(traded_at) else {
    return traded_at.to_owned();
  };
  let seoul = at.with_timezone(&FixedOffset::east_opt(9 * 3600).expect("Seoul's offset is an offset"));
  let kst = format!("{} KST", seoul.format("%Y-%m-%d %H:%M"));

  if traded_at.ends_with("+09:00") {
    kst
  } else {
    format!(
      "{kst} ({place} {} {})",
      &traded_at[..10.min(traded_at.len())],
      traded_at.get(11..16).unwrap_or("")
    )
  }
}

pub fn signed(value: f64, text: String) -> String {
  if value > 0.0 { format!("+{text}") } else { text }
}

/// The day a row names, the first ten characters of its time where they are a date.
pub fn date_of(value: &Value) -> Option<String> {
  let text = value.as_str()?;
  let date = text.get(..10)?;
  let bytes = date.as_bytes();
  let shaped = bytes.len() == 10
    && bytes[4] == b'-'
    && bytes[7] == b'-'
    && bytes.iter().enumerate().all(|(at, byte)| at == 4 || at == 7 || byte.is_ascii_digit());

  shaped.then(|| date.to_owned())
}

/// Where an item trades, which is what its addresses and its calendar hang on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StockMarket {
  Korean,
  World,
}

impl StockMarket {
  /// The clock a quote's time is named by beside KST: the world host serves exchanges other than New York's,
  /// so its clock is named for no one city.
  pub fn place(self) -> &'static str {
    match self {
      Self::Korean => "Seoul",
      Self::World => "local",
    }
  }

  pub fn price_url(self, code: &str, days: u32) -> String {
    match self {
      Self::Korean => format!("https://m.stock.naver.com/api/stock/{code}/price?pageSize={days}&page=1"),
      Self::World => format!("https://api.stock.naver.com/stock/{code}/price?pageSize={days}&page=1"),
    }
  }

  pub fn quote_url(self, code: &str) -> String {
    match self {
      Self::Korean => format!("https://m.stock.naver.com/api/stock/{code}/basic"),
      Self::World => format!("https://api.stock.naver.com/stock/{code}/basic"),
    }
  }

  pub fn page_url(self, code: &str) -> String {
    match self {
      Self::Korean => format!("https://m.stock.naver.com/domestic/stock/{code}"),
      Self::World => format!("https://m.stock.naver.com/worldstock/stock/{code}"),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StockItem {
  /// The Korean code or the Reuters code the item's addresses are made with.
  pub code: String,
  pub market: StockMarket,
}

static KOREAN_CODE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^[0-9][0-9A-Z]{5}$").expect("a fixed pattern"));
// A Reuters code of an American listing: the ticker and its exchange (O NASDAQ, N NYSE, K and A the AMEX).
static REUTERS_CODE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^[A-Z0-9]+\.[ONKA]$").expect("a fixed pattern"));

/// The item a query names outright, with no search: a Korean code or a Reuters code.
pub fn stock_item_of(query: &str) -> Option<StockItem> {
  let text = query.trim().to_uppercase();

  if KOREAN_CODE.is_match(&text) {
    return Some(StockItem {
      code: text,
      market: StockMarket::Korean,
    });
  }

  REUTERS_CODE.is_match(&text).then_some(StockItem {
    code: text,
    market: StockMarket::World,
  })
}

pub fn stock_search_url(query: &str) -> String {
  format!("https://ac.stock.naver.com/ac?q={}&target=stock", url_component(query.trim()))
}

/// The item the Naver stock site's search box offers for a ticker or a name: the one whose code is the query, otherwise
/// the first it names.
pub fn parse_stock_search(body: &str, query: &str) -> Option<StockItem> {
  let parsed: Value = serde_json::from_str(body).ok()?;
  let items = parsed["items"].as_array()?;
  let wanted = query.trim().to_uppercase();
  let offered: Vec<&Value> = items
    .iter()
    .filter(|item| item["code"].is_string() && item["reutersCode"].is_string())
    .collect();
  let text = |item: &Value, key: &str| item[key].as_str().unwrap_or_default().to_owned();
  let item = offered
    .iter()
    .find(|each| {
      text(each, "code").to_uppercase() == wanted || text(each, "reutersCode").to_uppercase() == wanted
    })
    .or(offered.first())?;

  Some(if item["nationCode"] == "KOR" {
    StockItem {
      code: text(item, "code"),
      market: StockMarket::Korean,
    }
  } else {
    StockItem {
      code: text(item, "reutersCode"),
      market: StockMarket::World,
    }
  })
}
