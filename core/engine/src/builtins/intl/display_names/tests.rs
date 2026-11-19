use crate::{JsNativeErrorKind, JsValue, TestAction, js_string, run_test_actions};

/// Region, language, script and currency codes are named in the locale.
#[test]
fn names_codes() {
    run_test_actions([
        TestAction::assert_eq(
            "new Intl.DisplayNames(['en'], { type: 'region' }).of('JP')",
            js_string!("Japan"),
        ),
        TestAction::assert_eq(
            "new Intl.DisplayNames(['fr'], { type: 'region' }).of('US')",
            js_string!("États-Unis"),
        ),
        TestAction::assert_eq(
            "new Intl.DisplayNames(['en'], { type: 'language' }).of('fr')",
            js_string!("French"),
        ),
        TestAction::assert_eq(
            "new Intl.DisplayNames(['en'], { type: 'script' }).of('Latn')",
            js_string!("Latin"),
        ),
        TestAction::assert_eq(
            "new Intl.DisplayNames(['en'], { type: 'currency' }).of('eur')",
            js_string!("Euro"),
        ),
    ]);
}

/// An unknown but well-formed code falls back to the canonical code, or
/// to `undefined` under `fallback: 'none'`.
#[test]
fn fallback() {
    run_test_actions([
        TestAction::assert_eq(
            "new Intl.DisplayNames(['en'], { type: 'region' }).of('aa')",
            js_string!("AA"),
        ),
        TestAction::assert_eq(
            "new Intl.DisplayNames(['en'], { type: 'region', fallback: 'none' }).of('AA')",
            JsValue::undefined(),
        ),
        TestAction::assert_eq(
            "new Intl.DisplayNames(['en'], { type: 'calendar' }).of('gregory')",
            js_string!("gregory"),
        ),
    ]);
}

/// A missing `type`, or a code malformed for the type, throws.
#[test]
fn errors() {
    run_test_actions([
        TestAction::assert_native_error(
            "new Intl.DisplayNames(['en'], {})",
            JsNativeErrorKind::Type,
            "Intl.DisplayNames needs a type",
        ),
        TestAction::assert_native_error(
            "new Intl.DisplayNames(['en'], { type: 'currency' }).of('EURO')",
            JsNativeErrorKind::Range,
            "invalid code for Intl.DisplayNames.prototype.of",
        ),
    ]);
}

/// `resolvedOptions` reports the defaults it filled in.
#[test]
fn resolved_options() {
    run_test_actions([TestAction::assert_eq(
        indoc::indoc! {"
            const o = new Intl.DisplayNames(['en'], { type: 'region' }).resolvedOptions();
            [o.locale, o.style, o.type, o.fallback].join(',')
        "},
        js_string!("en,long,region,code"),
    )]);
}
