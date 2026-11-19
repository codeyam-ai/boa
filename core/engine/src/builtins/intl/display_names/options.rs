//! The `Intl.DisplayNames` options (`type`, `style`, `fallback`,
//! `languageDisplay`) and the code canonicalization `of()` applies.

use cow_utils::CowUtils;
use icu_locale::{
    Locale,
    subtags::{Region, Script},
};

use crate::{Context, JsResult, JsValue, builtins::options::OptionType, js_error};

/// The `type` option: which kind of code `of()` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DisplayType {
    Language,
    Region,
    Script,
    Currency,
    Calendar,
    DateTimeField,
}

impl DisplayType {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Language => "language",
            Self::Region => "region",
            Self::Script => "script",
            Self::Currency => "currency",
            Self::Calendar => "calendar",
            Self::DateTimeField => "dateTimeField",
        }
    }
}

impl OptionType for DisplayType {
    fn from_value(value: JsValue, context: &mut Context) -> JsResult<Self> {
        match value.to_string(context)?.to_std_string_escaped().as_str() {
            "language" => Ok(Self::Language),
            "region" => Ok(Self::Region),
            "script" => Ok(Self::Script),
            "currency" => Ok(Self::Currency),
            "calendar" => Ok(Self::Calendar),
            "dateTimeField" => Ok(Self::DateTimeField),
            _ => Err(js_error!(RangeError: "unknown Intl.DisplayNames type")),
        }
    }
}

/// The `style` option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DisplayStyle {
    Narrow,
    Short,
    Long,
}

impl DisplayStyle {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Narrow => "narrow",
            Self::Short => "short",
            Self::Long => "long",
        }
    }
}

impl OptionType for DisplayStyle {
    fn from_value(value: JsValue, context: &mut Context) -> JsResult<Self> {
        match value.to_string(context)?.to_std_string_escaped().as_str() {
            "narrow" => Ok(Self::Narrow),
            "short" => Ok(Self::Short),
            "long" => Ok(Self::Long),
            _ => Err(js_error!(RangeError: "unknown Intl.DisplayNames style")),
        }
    }
}

/// The `fallback` option: what `of()` returns for a code with no name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DisplayFallback {
    Code,
    None,
}

impl OptionType for DisplayFallback {
    fn from_value(value: JsValue, context: &mut Context) -> JsResult<Self> {
        match value.to_string(context)?.to_std_string_escaped().as_str() {
            "code" => Ok(Self::Code),
            "none" => Ok(Self::None),
            _ => Err(js_error!(RangeError: "unknown Intl.DisplayNames fallback")),
        }
    }
}

/// The `languageDisplay` option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DisplayLanguage {
    Dialect,
    Standard,
}

impl OptionType for DisplayLanguage {
    fn from_value(value: JsValue, context: &mut Context) -> JsResult<Self> {
        match value.to_string(context)?.to_std_string_escaped().as_str() {
            "dialect" => Ok(Self::Dialect),
            "standard" => Ok(Self::Standard),
            _ => Err(js_error!(RangeError: "unknown Intl.DisplayNames languageDisplay")),
        }
    }
}

/// [`CanonicalCodeForDisplayNames ( type, code )`][spec]: the code `of()`
/// looks up and, with `fallback: "code"`, returns. Throws a `RangeError`
/// for a code that is not well formed for the type.
///
/// [spec]: https://tc39.es/ecma402/#sec-canonicalcodefordisplaynames
pub(super) fn canonical_code(typ: DisplayType, code: &str) -> JsResult<String> {
    let invalid = || js_error!(RangeError: "invalid code for Intl.DisplayNames.prototype.of");
    match typ {
        DisplayType::Language => {
            let locale = Locale::try_from_str(code).map_err(|_| invalid())?;
            // A language code is a unicode_language_id: no extensions.
            if !locale.extensions.is_empty() || code.contains("-u-") {
                return Err(invalid());
            }
            Ok(locale.id.to_string())
        }
        DisplayType::Region => {
            let region = Region::try_from_str(code).map_err(|_| invalid())?;
            Ok(region.to_string())
        }
        DisplayType::Script => {
            let script = Script::try_from_str(code).map_err(|_| invalid())?;
            Ok(script.to_string())
        }
        DisplayType::Currency => {
            if code.len() == 3 && code.bytes().all(|b| b.is_ascii_alphabetic()) {
                Ok(code.cow_to_ascii_uppercase().into_owned())
            } else {
                Err(invalid())
            }
        }
        DisplayType::Calendar => {
            let ok = !code.is_empty()
                && code.split('-').all(|part| {
                    (3..=8).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_alphanumeric())
                });
            if ok {
                Ok(code.cow_to_ascii_lowercase().into_owned())
            } else {
                Err(invalid())
            }
        }
        DisplayType::DateTimeField => {
            const FIELDS: [&str; 10] = [
                "era",
                "year",
                "quarter",
                "month",
                "weekOfYear",
                "weekday",
                "day",
                "dayPeriod",
                "hour",
                "minute",
            ];
            if FIELDS.contains(&code) || code == "second" || code == "timeZoneName" {
                Ok(code.to_owned())
            } else {
                Err(invalid())
            }
        }
    }
}
