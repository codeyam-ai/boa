use crate::builtins::intl::number_format::RoundingIncrement;
use fixed_decimal::RoundingIncrement::*;

#[test]
fn u16_to_rounding_increment_sunny_day() {
    #[rustfmt::skip]
    let valid_cases: [(u16, RoundingIncrement); 15] = [
        // Singles
        (1, RoundingIncrement::from_parts(MultiplesOf1, 0).unwrap()),
        (2, RoundingIncrement::from_parts(MultiplesOf2, 0).unwrap()),
        (5, RoundingIncrement::from_parts(MultiplesOf5, 0).unwrap()),
        // Tens
        (10, RoundingIncrement::from_parts(MultiplesOf1, 1).unwrap()),
        (20, RoundingIncrement::from_parts(MultiplesOf2, 1).unwrap()),
        (25, RoundingIncrement::from_parts(MultiplesOf25, 0).unwrap()),
        (50, RoundingIncrement::from_parts(MultiplesOf5, 1).unwrap()),
        // Hundreds
        (100, RoundingIncrement::from_parts(MultiplesOf1, 2).unwrap()),
        (200, RoundingIncrement::from_parts(MultiplesOf2, 2).unwrap()),
        (250, RoundingIncrement::from_parts(MultiplesOf25, 1).unwrap()),
        (500, RoundingIncrement::from_parts(MultiplesOf5, 2).unwrap()),
        // Thousands
        (1000, RoundingIncrement::from_parts(MultiplesOf1, 3).unwrap()),
        (2000, RoundingIncrement::from_parts(MultiplesOf2, 3).unwrap()),
        (2500, RoundingIncrement::from_parts(MultiplesOf25, 2).unwrap()),
        (5000, RoundingIncrement::from_parts(MultiplesOf5, 3).unwrap()),
    ];

    for (num, increment) in valid_cases {
        assert_eq!(RoundingIncrement::from_u16(num), Some(increment));
    }
}

#[test]
fn u16_to_rounding_increment_rainy_day() {
    const INVALID_CASES: [u16; 9] = [0, 4, 6, 24, 10000, 65535, 7373, 140, 1500];

    for num in INVALID_CASES {
        assert!(RoundingIncrement::from_u16(num).is_none());
    }
}

#[cfg(feature = "intl_bundled")]
mod styles {
    use crate::{TestAction, js_string, run_test_actions};

    /// Currency: the symbol and its placement follow the locale, the
    /// fraction digits follow the currency unless overridden.
    #[test]
    fn currency_style() {
        run_test_actions([
            TestAction::assert_eq(
                "new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD' }).format(1234.5)",
                js_string!("$1,234.50"),
            ),
            TestAction::assert_eq(
                "new Intl.NumberFormat('en-US', { style: 'currency', currency: 'JPY' }).format(1234.5)",
                js_string!("¥1,235"),
            ),
            TestAction::assert_eq(
                "new Intl.NumberFormat('de-DE', { style: 'currency', currency: 'EUR' }).format(1234.5)",
                js_string!("1.234,50\u{a0}€"),
            ),
            TestAction::assert_eq(
                "new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', maximumFractionDigits: 0 }).format(9.99)",
                js_string!("$10"),
            ),
            TestAction::assert_eq(
                "new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', useGrouping: false }).format(1234.5)",
                js_string!("$1234.50"),
            ),
            TestAction::assert_eq(
                "new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD' }).format(-5)",
                js_string!("-$5.00"),
            ),
        ]);
    }

    /// Percent multiplies by 100 and appends the locale's percent sign.
    #[test]
    fn percent_style() {
        run_test_actions([
            TestAction::assert_eq(
                "new Intl.NumberFormat('en-US', { style: 'percent' }).format(0.25)",
                js_string!("25%"),
            ),
            TestAction::assert_eq(
                "new Intl.NumberFormat('en-US', { style: 'percent', maximumFractionDigits: 1 }).format(0.1234)",
                js_string!("12.3%"),
            ),
        ]);
    }

    /// Unit names are short by default, spelled out for `long`, and the
    /// number keeps its own rounding.
    #[test]
    fn unit_style() {
        run_test_actions([
            TestAction::assert_eq(
                "new Intl.NumberFormat('en-US', { style: 'unit', unit: 'kilogram', maximumFractionDigits: 1 }).format(2.25)",
                js_string!("2.3 kg"),
            ),
            TestAction::assert_eq(
                "new Intl.NumberFormat('en-US', { style: 'unit', unit: 'liter', unitDisplay: 'long' }).format(3)",
                js_string!("3 liters"),
            ),
            TestAction::assert_eq(
                "new Intl.NumberFormat('en-US', { style: 'unit', unit: 'kilometer', unitDisplay: 'long' }).format(1)",
                js_string!("1 kilometer"),
            ),
        ]);
    }
}
