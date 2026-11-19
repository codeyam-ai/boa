use indoc::indoc;

use crate::{TestAction, js_string, run_test_actions};

// Intl.DateTimeFormat tests

#[cfg(feature = "intl_bundled")]
#[test]
fn dtf_basic() {
    run_test_actions([
        TestAction::run(indoc! {"
            // Setup date
            const date = new Date(Date.UTC(2020, 11, 20, 3, 23, 16, 738));

            let formatter = new Intl.DateTimeFormat('en-US');
            let result = formatter.format(date);
        "}),
        // The default year is `numeric`: the full year, as browsers print it.
        TestAction::assert_eq("result === '12/20/2020'", true),
    ]);
    run_test_actions([
        TestAction::run(indoc! {"
            // Setup date
            const date = new Date(Date.UTC(2020, 11, 20, 3, 23, 16, 738));

            let formatter = new Intl.DateTimeFormat('en-US', { dateStyle: 'full' });
            let result = formatter.format(date);
        "}),
        TestAction::assert_eq("result === 'Sunday, December 20, 2020'", true),
    ]);
    run_test_actions([
        TestAction::run(indoc! {"
            // Setup date
            const date = new Date(Date.UTC(2020, 11, 20, 3, 23, 16, 738));

            let formatter = new Intl.DateTimeFormat('en-GB');
            let result = formatter.format(date);
        "}),
        TestAction::assert_eq("result === '20/12/2020'", true),
    ]);
    run_test_actions([
        TestAction::run(indoc! {"
            // Setup date
            const date = new Date(Date.UTC(2020, 11, 20, 3, 23, 16, 738));

            let formatter = new Intl.DateTimeFormat('en-GB', {
                dateStyle: 'full',
                timeStyle: 'long',
            });
            let result = formatter.format(date);
        "}),
        TestAction::assert_eq("result === 'Sunday, 20 December 2020 at 03:23:16'", true),
    ]);
    run_test_actions([
        TestAction::run(indoc! {"
            // Setup date
            const date = new Date(Date.UTC(2020, 11, 20, 3, 23, 16, 738));

            let formatter = new Intl.DateTimeFormat('en-GB', {
                dateStyle: 'full',
                timeStyle: 'long',
                timeZone: 'Australia/Sydney',
            });
            let result = formatter.format(date);
        "}),
        TestAction::assert_eq("result === 'Sunday, 20 December 2020 at 14:23:16'", true),
    ]);
}

#[cfg(feature = "intl_bundled")]
#[test]
fn date_to_locale_string() {
    run_test_actions([
        TestAction::run(indoc! {"
            // Setup date
            const date = new Date(Date.UTC(2021, 3, 12, 6, 7));

            let result = date.toLocaleString('en-US', { dateStyle: 'short' });
        "}),
        TestAction::assert_eq("result === '4/12/21'", true),
    ]);
    run_test_actions([
        TestAction::run(indoc! {"
            // Setup date
            const date = new Date(Date.UTC(2021, 3, 12, 6, 7));

            let result = date.toLocaleString('en-US', { timeStyle: 'short' });
        "}),
        TestAction::assert_eq("result === '6:07\u{202f}AM'", true),
    ]);
}

#[cfg(feature = "intl_bundled")]
#[test]
fn dtf_ctor_observable_behavior() {
    run_test_actions([
        TestAction::run(indoc! {"
            const expected = [];

            const proxyConstructor = new Proxy(Intl.DateTimeFormat, {
              get(target, prop) {
                if (prop === 'prototype') {
                  expected.push('prototype-access');
                }
                return target[prop];
              }
            });

            try {
              new proxyConstructor('en', { timeZone: 'Invalid/Zone' });
            } catch (e) {
              expected.push('error-thrown');
            }
        "}),
        TestAction::assert_eq("expected.length === 2", true),
        TestAction::assert_eq("expected[0] === 'prototype-access'", true),
        TestAction::assert_eq("expected[1] === 'error-thrown'", true),
    ]);
}

/// `formatToParts` splits a formatted date into typed pieces with the
/// separators between them as `literal`.
#[cfg(feature = "intl_bundled")]
#[test]
fn dtf_format_to_parts() {
    run_test_actions([TestAction::assert_eq(
        indoc! {"
            new Intl.DateTimeFormat('en-US', { month: 'long', year: 'numeric', timeZone: 'UTC' })
                .formatToParts(Date.UTC(2026, 10, 1))
                .map(p => p.type + ':' + p.value).join(',')
        "},
        js_string!("month:November,literal: ,year:2026"),
    )]);
}

/// `formatRange` joins two different days with an en dash, and collapses
/// two dates that format alike into one; `formatRangeToParts` tags each
/// piece with its source.
#[cfg(feature = "intl_bundled")]
#[test]
fn dtf_format_range() {
    run_test_actions([
        TestAction::run(
            "const f = new Intl.DateTimeFormat('en-US', { month: 'short', day: 'numeric', timeZone: 'UTC' });",
        ),
        TestAction::assert_eq(
            "f.formatRange(Date.UTC(2026, 10, 23), Date.UTC(2026, 10, 26))",
            js_string!("Nov 23\u{2009}\u{2013}\u{2009}Nov 26"),
        ),
        TestAction::assert_eq(
            "f.formatRange(Date.UTC(2026, 10, 23, 1), Date.UTC(2026, 10, 23, 9))",
            js_string!("Nov 23"),
        ),
        TestAction::assert_eq(
            "[...new Set(f.formatRangeToParts(Date.UTC(2026, 10, 23), Date.UTC(2026, 10, 26)).map(p => p.source))].join(',')",
            js_string!("startRange,shared,endRange"),
        ),
        TestAction::assert_native_error(
            "f.formatRange(Date.UTC(2026, 10, 23))",
            crate::JsNativeErrorKind::Type,
            "formatRange needs a start and an end date",
        ),
    ]);
}

/// `resolvedOptions().timeZone` reports the IANA name in its canonical
/// case, whatever case the option was given in.
#[cfg(feature = "intl_bundled")]
#[test]
fn dtf_time_zone_case_normalized() {
    run_test_actions([TestAction::assert_eq(
        "new Intl.DateTimeFormat('en-US', { timeZone: 'america/new_york' }).resolvedOptions().timeZone",
        js_string!("America/New_York"),
    )]);
}
