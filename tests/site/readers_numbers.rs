use bmo_explorer::site::readers::numbers::*;

#[test]
fn a_number_is_grouped_by_thousands_with_its_decimals_trimmed() {
  assert_eq!(grouped(1_236_890.0), "1,236,890");
  assert_eq!(grouped(1_385.8), "1,385.8");
  assert_eq!(grouped(0.12345), "0.123");
  assert_eq!(grouped(-1_234.5), "-1,234.5");
  assert_eq!(grouped(100.0), "100");
  assert_eq!(figure("1,385.80"), Some(1_385.8));
  assert_eq!(figure(""), None);
}

#[test]
fn a_fixed_number_rounds_its_exact_value_a_tie_away_from_zero() {
  assert_eq!(fixed(4.114, 3), "4.114");
  assert_eq!(fixed(0.125, 2), "0.13");
  assert_eq!(fixed(-0.01, 3), "-0.010");
  assert_eq!(fixed(1.005, 2), "1.00");
  assert_eq!(fixed(-1.81, 2), "-1.81");
  assert_eq!(localized(7_080.92, 2, 2), "7,080.92");
  assert_eq!(localized(-170.79, 2, 2), "-170.79");
  assert_eq!(localized(341.07, 2, 4), "341.07");
  assert_eq!(localized(271_500.0, 0, 4), "271,500");
  assert_eq!(plain(67.0), "67");
}

#[test]
fn a_number_in_a_radix_is_written_as_the_browsers_write_it() {
  assert_eq!(in_radix(2_101_917_379_785_838_660.0 / 1e15 * std::f64::consts::PI, 36), "53f.d96okpmq");
  assert_eq!(in_radix(123_456_789_012_345.0 / 1e15 * std::f64::consts::PI, 36), "0.dynkna8f4fr");
  assert_eq!(in_radix(20.0 / 1e15 * std::f64::consts::PI, 36), "0.000000006dq1a2xwd93");
  assert_eq!(in_radix(0.1, 36), "0.3lllllllllm");
  assert_eq!(in_radix(1.0 / 3.0, 36), "0.c");
  assert_eq!(in_radix(123.456, 36), "3f.gez4w97ry");
  assert_eq!(in_radix(2f64.powi(60), 36), "8rc4kbdvss00");
  assert_eq!(in_radix(-255.0, 16), "-ff");
}
