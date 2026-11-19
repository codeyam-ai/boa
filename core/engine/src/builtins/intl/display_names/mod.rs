//! This module implements the global `Intl.DisplayNames` object.
//!
//! `Intl.DisplayNames` translates the codes of languages, regions, scripts
//! and currencies into their names in a locale: `of("US")` is
//! `"United States"` in English and `"États-Unis"` in French.
//!
//! More information:
//!  - [ECMAScript reference][spec]
//!
//! [spec]: https://tc39.es/ecma402/#intl-displaynames-objects

use boa_gc::{Finalize, Trace};
use icu_experimental::{
    dimension::provider::currency::displayname::CurrencyDisplaynameV1,
    displaynames::{
        DisplayNamesOptions, DisplayNamesPreferences, LanguageDisplay, Style,
        multi::{LocaleDisplayNamesFormatter, RegionDisplayNames, ScriptDisplayNames},
        provider::RegionDisplayNamesV1,
    },
};
use icu_locale::{
    Locale,
    subtags::{Region, Script},
};
use icu_provider::{
    DataIdentifierBorrowed, DataLocale, DataMarkerAttributes, DataProvider, DataRequest,
    ResultDataError,
};

use super::{
    Service,
    locale::{canonicalize_locale_list, filter_locales, resolve_locale},
    options::{EmptyPreferences, IntlOptions},
};
use crate::{
    Context, JsArgs, JsData, JsNativeError, JsResult, JsString, JsValue,
    builtins::{
        BuiltInBuilder, BuiltInConstructor, BuiltInObject, IntrinsicObject,
        options::{get_option, get_options_object},
    },
    context::intrinsics::{Intrinsics, StandardConstructor, StandardConstructors},
    js_error, js_string,
    object::{JsObject, ObjectInitializer, internal_methods::get_prototype_from_constructor},
    property::Attribute,
    realm::Realm,
    string::StaticJsStrings,
    symbol::JsSymbol,
};

mod options;

#[cfg(all(test, feature = "intl_bundled"))]
mod tests;

use options::{DisplayFallback, DisplayLanguage, DisplayStyle, DisplayType, canonical_code};

/// The CLDR names one `Intl.DisplayNames` reads from.
#[derive(Debug)]
enum Names {
    /// Boxed: the formatter is several times the size of the other variants.
    Language(Box<LocaleDisplayNamesFormatter>),
    Region(RegionDisplayNames),
    Script(ScriptDisplayNames),
    /// Currency names are read per code, in this locale.
    Currency(DataLocale),
    /// No CLDR names are bundled for calendars or date-time fields; `of()`
    /// answers with its fallback.
    None,
}

/// JavaScript `Intl.DisplayNames` object.
#[derive(Debug, Trace, Finalize, JsData)]
// Safety: `DisplayNames` only contains non-traceable types.
#[boa_gc(unsafe_empty_trace)]
pub(crate) struct DisplayNames {
    locale: Locale,
    style: DisplayStyle,
    typ: DisplayType,
    fallback: DisplayFallback,
    language_display: Option<DisplayLanguage>,
    names: Names,
}

impl Service for DisplayNames {
    type LangMarker = RegionDisplayNamesV1;

    type Preferences = EmptyPreferences;
}

impl IntrinsicObject for DisplayNames {
    fn init(realm: &Realm) {
        BuiltInBuilder::from_standard_constructor::<Self>(realm)
            .static_method(
                Self::supported_locales_of,
                js_string!("supportedLocalesOf"),
                1,
            )
            .property(
                JsSymbol::to_string_tag(),
                js_string!("Intl.DisplayNames"),
                Attribute::CONFIGURABLE,
            )
            .method(Self::of, js_string!("of"), 1)
            .method(Self::resolved_options, js_string!("resolvedOptions"), 0)
            .build();
    }

    fn get(intrinsics: &Intrinsics) -> JsObject {
        Self::STANDARD_CONSTRUCTOR(intrinsics.constructors()).constructor()
    }
}

impl BuiltInObject for DisplayNames {
    const NAME: JsString = StaticJsStrings::DISPLAY_NAMES;
}

impl BuiltInConstructor for DisplayNames {
    const CONSTRUCTOR_ARGUMENTS: usize = 2;
    const PROTOTYPE_STORAGE_SLOTS: usize = 3;
    const CONSTRUCTOR_STORAGE_SLOTS: usize = 1;

    const STANDARD_CONSTRUCTOR: fn(&StandardConstructors) -> &StandardConstructor =
        StandardConstructors::display_names;

    /// [`Intl.DisplayNames ( locales, options )`][spec].
    ///
    /// [spec]: https://tc39.es/ecma402/#sec-Intl.DisplayNames
    fn constructor(
        new_target: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        // 1. If NewTarget is undefined, throw a TypeError exception.
        if new_target.is_undefined() {
            return Err(JsNativeError::typ()
                .with_message("cannot call `Intl.DisplayNames` constructor without `new`")
                .into());
        }
        // 2. Let displayNames be ? OrdinaryCreateFromConstructor(NewTarget, "%Intl.DisplayNames.prototype%", …).
        let prototype = get_prototype_from_constructor(
            new_target,
            StandardConstructors::display_names,
            context,
        )?;

        // 3. Let optionsResolution be ? ResolveOptions(%Intl.DisplayNames%, …, locales, options, « require-options »).
        let requested_locales = canonicalize_locale_list(args.get_or_undefined(0), context)?;
        let options = args.get_or_undefined(1);
        if options.is_undefined() {
            return Err(
                js_error!(TypeError: "Intl.DisplayNames needs an options object with a type"),
            );
        }
        let options = get_options_object(options)?;
        let matcher =
            get_option(&options, js_string!("localeMatcher"), context)?.unwrap_or_default();
        let locale = resolve_locale::<Self>(
            requested_locales,
            &mut IntlOptions {
                matcher,
                ..Default::default()
            },
            context.intl_provider(),
        )?;

        // 6. Let style be ? GetOption(options, "style", string, « "narrow", "short", "long" », "long").
        let style =
            get_option(&options, js_string!("style"), context)?.unwrap_or(DisplayStyle::Long);
        // 8. Let type be ? GetOption(options, "type", string, « … », undefined).
        // 9. If type is undefined, throw a TypeError exception.
        let typ: DisplayType = get_option(&options, js_string!("type"), context)?
            .ok_or_else(|| js_error!(TypeError: "Intl.DisplayNames needs a type"))?;
        // 11. Let fallback be ? GetOption(options, "fallback", string, « "code", "none" », "code").
        let fallback =
            get_option(&options, js_string!("fallback"), context)?.unwrap_or(DisplayFallback::Code);
        // 13. Let languageDisplay be ? GetOption(options, "languageDisplay", string, « "dialect", "standard" », "dialect").
        let language_display: DisplayLanguage =
            get_option(&options, js_string!("languageDisplay"), context)?
                .unwrap_or(DisplayLanguage::Dialect);

        let mut icu_options = DisplayNamesOptions::default();
        icu_options.style = Some(match style {
            DisplayStyle::Narrow => Style::Narrow,
            DisplayStyle::Short => Style::Short,
            DisplayStyle::Long => Style::Long,
        });
        icu_options.language_display = match language_display {
            DisplayLanguage::Dialect => LanguageDisplay::Dialect,
            DisplayLanguage::Standard => LanguageDisplay::Standard,
        };
        let prefs = DisplayNamesPreferences::from(&locale);
        let provider = context.intl_provider();
        let data_error =
            |e: icu_provider::DataError| JsNativeError::range().with_message(e.to_string());
        let names = match typ {
            DisplayType::Language => Names::Language(Box::new(
                LocaleDisplayNamesFormatter::try_new_unstable(provider, prefs, icu_options)
                    .map_err(data_error)?,
            )),
            DisplayType::Region => Names::Region(
                RegionDisplayNames::try_new_unstable(provider, prefs, icu_options)
                    .map_err(data_error)?,
            ),
            DisplayType::Script => Names::Script(
                ScriptDisplayNames::try_new_unstable(provider, prefs, icu_options)
                    .map_err(data_error)?,
            ),
            DisplayType::Currency => Names::Currency(DataLocale::from(&locale)),
            DisplayType::Calendar | DisplayType::DateTimeField => Names::None,
        };

        let display_names = Self {
            locale,
            style,
            typ,
            fallback,
            // 24. If type is "language", set displayNames.[[LanguageDisplay]] to languageDisplay.
            language_display: (typ == DisplayType::Language).then_some(language_display),
            names,
        };
        Ok(JsObject::from_proto_and_data_with_shared_shape(
            context.root_shape(),
            prototype,
            display_names,
        )
        .into())
    }
}

impl DisplayNames {
    /// [`Intl.DisplayNames.supportedLocalesOf ( locales [ , options ] )`][spec].
    ///
    /// [spec]: https://tc39.es/ecma402/#sec-Intl.DisplayNames.supportedLocalesOf
    fn supported_locales_of(
        _: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        let requested_locales = canonicalize_locale_list(args.get_or_undefined(0), context)?;
        filter_locales::<Self>(requested_locales, args.get_or_undefined(1), context)
            .map(JsValue::from)
    }

    /// The CLDR name for the canonical `code`, if there is one.
    fn name_for(&self, code: &str, context: &Context) -> Option<String> {
        match &self.names {
            Names::Language(formatter) => {
                let locale = Locale::try_from_str(code).ok()?;
                let name = formatter.of(&locale).into_owned();
                // The formatter echoes an unknown language back unchanged.
                (name != code && name != locale.to_string()).then_some(name)
            }
            Names::Region(names) => names
                .of(Region::try_from_str(code).ok()?)
                .map(str::to_owned),
            Names::Script(names) => names
                .of(Script::try_from_str(code).ok()?)
                .map(str::to_owned),
            Names::Currency(locale) => {
                let attributes = DataMarkerAttributes::try_from_str(code).ok()?;
                let res = DataProvider::<CurrencyDisplaynameV1>::load(
                    context.intl_provider(),
                    DataRequest {
                        id: DataIdentifierBorrowed::for_marker_attributes_and_locale(
                            attributes, locale,
                        ),
                        ..Default::default()
                    },
                )
                .allow_identifier_not_found()
                .ok()??;
                Some(res.payload.get().to_string())
            }
            Names::None => None,
        }
    }

    /// [`Intl.DisplayNames.prototype.of ( code )`][spec].
    ///
    /// [spec]: https://tc39.es/ecma402/#sec-Intl.DisplayNames.prototype.of
    fn of(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        // 1. Let displayNames be this value.
        // 2. Perform ? RequireInternalSlot(displayNames, [[InitializedDisplayNames]]).
        let object = this.as_object();
        let display_names = object
            .as_ref()
            .and_then(|o| o.downcast_ref::<Self>())
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("`of` can only be called on an `Intl.DisplayNames` object")
            })?;
        // 3. Let code be ? ToString(code).
        let code = args
            .get_or_undefined(0)
            .to_string(context)?
            .to_std_string_escaped();
        // 4. Set code to ? CanonicalCodeForDisplayNames(displayNames.[[Type]], code).
        let code = canonical_code(display_names.typ, &code)?;
        // 5–6. Let result be the name for code, if the locale data has one; return it.
        if let Some(name) = display_names.name_for(&code, context) {
            return Ok(js_string!(name.as_str()).into());
        }
        // 7. If displayNames.[[Fallback]] is "code", return code.
        // 8. Return undefined.
        Ok(match display_names.fallback {
            DisplayFallback::Code => js_string!(code.as_str()).into(),
            DisplayFallback::None => JsValue::undefined(),
        })
    }

    /// [`Intl.DisplayNames.prototype.resolvedOptions ( )`][spec].
    ///
    /// [spec]: https://tc39.es/ecma402/#sec-Intl.DisplayNames.prototype.resolvedOptions
    fn resolved_options(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let object = this.as_object();
        let display_names = object
            .as_ref()
            .and_then(|o| o.downcast_ref::<Self>())
            .ok_or_else(|| {
                JsNativeError::typ().with_message(
                    "`resolvedOptions` can only be called on an `Intl.DisplayNames` object",
                )
            })?;
        let (locale, style, typ, fallback, language_display) = {
            let locale = display_names.locale.to_string();
            let style = display_names.style.as_str();
            let typ = display_names.typ.as_str();
            let fallback = match display_names.fallback {
                DisplayFallback::Code => "code",
                DisplayFallback::None => "none",
            };
            let language_display = display_names.language_display.map(|d| match d {
                DisplayLanguage::Dialect => "dialect",
                DisplayLanguage::Standard => "standard",
            });
            (locale, style, typ, fallback, language_display)
        };
        drop(display_names);

        let mut options = ObjectInitializer::new(context);
        options
            .property(
                js_string!("locale"),
                js_string!(locale.as_str()),
                Attribute::all(),
            )
            .property(js_string!("style"), js_string!(style), Attribute::all())
            .property(js_string!("type"), js_string!(typ), Attribute::all())
            .property(
                js_string!("fallback"),
                js_string!(fallback),
                Attribute::all(),
            );
        if let Some(language_display) = language_display {
            options.property(
                js_string!("languageDisplay"),
                js_string!(language_display),
                Attribute::all(),
            );
        }
        Ok(options.build().into())
    }
}
