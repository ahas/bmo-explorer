//! Numbers as the tools write them, the way a page's own JavaScript writes them: `toFixed`, and
//! `toLocaleString` with its grouping and its fraction digits. Both round the number's exact value, a tie
//! away from zero.

// The number's digits rounded at `decimals`, a tie away from zero: its whole part and its fraction, without
// sign.
fn rounded(value: f64, decimals: usize) -> (String, String) {
  // Sixty decimals carry the exact value of every double this formatting meets far past any tie.
  let exact = format!("{:.60}", value.abs());
  let (whole, fraction) = exact.split_once('.').unwrap_or((&exact, ""));
  let mut digits: Vec<u8> =
    whole.bytes().chain(fraction.bytes().take(decimals)).map(|digit| digit - b'0').collect();
  let up = fraction.as_bytes().get(decimals).is_some_and(|next| *next >= b'5');

  if up {
    let mut at = digits.len();

    loop {
      if at == 0 {
        digits.insert(0, 1);
        break;
      }

      at -= 1;

      if digits[at] == 9 {
        digits[at] = 0;
      } else {
        digits[at] += 1;
        break;
      }
    }
  }

  let split = digits.len() - decimals;
  let text = |part: &[u8]| part.iter().map(|digit| char::from(b'0' + digit)).collect::<String>();

  (text(&digits[..split]), text(&digits[split..]))
}

fn sign(value: f64, whole: &str, fraction: &str) -> &'static str {
  let zero = whole.bytes().all(|digit| digit == b'0') && fraction.bytes().all(|digit| digit == b'0');

  if value < 0.0 && !zero { "-" } else { "" }
}

/// A number as `toFixed(decimals)` writes it.
pub fn fixed(value: f64, decimals: usize) -> String {
  if !value.is_finite() {
    return value.to_string();
  }

  let (whole, fraction) = rounded(value, decimals);
  let whole = whole.trim_start_matches('0');
  let whole = if whole.is_empty() { "0" } else { whole };
  let sign = if value < 0.0 { "-" } else { "" };

  if decimals == 0 {
    format!("{sign}{whole}")
  } else {
    format!("{sign}{whole}.{fraction}")
  }
}

/// A number as `toLocaleString` writes it with grouped thousands, at least `min` and at most `max` fraction
/// digits.
pub fn localized(value: f64, min: usize, max: usize) -> String {
  if !value.is_finite() {
    return value.to_string();
  }

  let (whole, fraction) = rounded(value, max);
  let whole = whole.trim_start_matches('0');
  let whole = if whole.is_empty() { "0" } else { whole };
  let mut fraction = fraction.trim_end_matches('0').to_owned();

  while fraction.len() < min {
    fraction.push('0');
  }

  let mut grouped = String::new();

  for (index, digit) in whole.chars().enumerate() {
    if index > 0 && (whole.len() - index) % 3 == 0 {
      grouped.push(',');
    }
    grouped.push(digit);
  }

  let sign = sign(value, whole, &fraction);

  if fraction.is_empty() {
    format!("{sign}{grouped}")
  } else {
    format!("{sign}{grouped}.{fraction}")
  }
}

/// A number with at most `decimals` decimals as `toLocaleString` writes it.
pub fn grouped_with(value: f64, decimals: usize) -> String {
  localized(value, 0, decimals)
}

/// A number as `toLocaleString("ko-KR")` writes it: grouped, at most three decimals.
pub fn grouped(value: f64) -> String {
  localized(value, 0, 3)
}

/// A number as JavaScript writes it into a template: no grouping, as few digits as say it.
pub fn plain(value: f64) -> String {
  if value.fract() == 0.0 && value.abs() < 1e21 {
    format!("{}", value as i64)
  } else {
    format!("{value}")
  }
}

/// A number as `toString(radix)` writes it in the browsers a page's script runs in: the whole part's digits, and
/// the fraction's digits only as far as the double's own precision reaches, the last rounded to even.
pub fn in_radix(value: f64, radix: u32) -> String {
  const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

  let base = f64::from(radix);
  let negative = value < 0.0;
  let value = value.abs();
  let mut integer = value.floor();
  let mut fraction = value - integer;
  let mut delta = (0.5 * (f64::from_bits(value.to_bits() + 1) - value)).max(f64::from_bits(1));
  let mut fractional: Vec<u8> = Vec::new();

  if fraction >= delta {
    loop {
      fraction *= base;
      delta *= base;

      let digit = fraction as usize;

      fractional.push(DIGITS[digit]);
      fraction -= digit as f64;

      if (fraction > 0.5 || (fraction == 0.5 && digit & 1 == 1)) && fraction + delta > 1.0 {
        // The rounding carries back over the digits already written, and past them into the whole part.
        loop {
          let Some(last) = fractional.pop() else {
            integer += 1.0;
            break;
          };
          let digit = DIGITS.iter().position(|one| *one == last).unwrap_or(0);

          if digit + 1 < radix as usize {
            fractional.push(DIGITS[digit + 1]);
            break;
          }
        }

        break;
      }

      if fraction < delta {
        break;
      }
    }
  }

  // Past the double's precision the whole part's digits are not represented, and are written as zeros.
  let mut whole: Vec<u8> = Vec::new();

  while ((integer / base).to_bits() >> 52 & 0x7ff) as i64 - 0x3ff - 52 > 0 {
    integer /= base;
    whole.push(b'0');
  }

  loop {
    let remainder = integer % base;

    whole.push(DIGITS[remainder as usize]);
    integer = (integer - remainder) / base;

    if integer <= 0.0 {
      break;
    }
  }

  whole.reverse();

  let mut text = String::from(if negative { "-" } else { "" });

  text.push_str(&String::from_utf8_lossy(&whole));

  if !fractional.is_empty() {
    text.push('.');
    text.push_str(&String::from_utf8_lossy(&fractional));
  }

  text
}

/// A figure written with grouping commas and perhaps a sign or decimals, read as a number.
pub fn figure(text: &str) -> Option<f64> {
  let cleaned: String = text.trim().chars().filter(|c| *c != ',').collect();

  if cleaned.is_empty() {
    return None;
  }

  cleaned.parse::<f64>().ok().filter(|value| value.is_finite())
}
