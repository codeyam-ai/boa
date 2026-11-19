//! The `style` option of `Intl.NumberFormat`: `percent`, `currency` and
//! `unit`.
//!
//! The number itself is always formatted by the `NumberFormat`'s own
//! decimal (or compact) formatter, so every digit option (`minimumFractionDigits`,
//! `useGrouping`, `notation`, `signDisplay`, …) applies under every style. This
//! module only supplies what surrounds the number: the locale's percent
//! pattern, the currency pattern with its symbol, code or plural name, or the
//! unit's plural pattern, all from CLDR through `icu_experimental`'s data.

use cow_utils::CowUtils;
use fixed_decimal::{Decimal, Sign};
use icu_experimental::dimension::{
    currency::CurrencyType,
    provider::{
        currency::{
            essentials::CurrencyEssentialsV1, extended::CurrencyExtendedDataV1,
            fractions::CurrencyFractionsV1, patterns::CurrencyPatternsDataV1,
            symbols::CurrencySymbolsV1,
        },
        percent::PercentEssentialsV1,
        units::{categorized_display_names as units, display_names::UnitsDisplayNames},
    },
};
use icu_locale::Locale;
use icu_plurals::{PluralOperands, PluralRules};
use icu_provider::{
    DataError, DataErrorKind, DataIdentifierBorrowed, DataLocale, DataMarker, DataMarkerAttributes,
    DataPayload, DataProvider, DataRequest, DataRequestMetadata, DataResponse, ResultDataError,
    marker::ErasedMarker,
};
use tinystr::TinyAsciiStr;
use writeable::Writeable;

use super::{CurrencyDisplay, CurrencySign, Unit, UnitDisplay, UnitFormatOptions};
use crate::context::icu::IntlProvider;

/// What a style puts around the formatted number.
#[derive(Debug)]
pub(crate) enum StyleFormatter {
    /// `style: "decimal"`: the number alone.
    Decimal,
    /// `style: "percent"`: the value times 100 in the locale's percent
    /// pattern (`25%`, `25 %`).
    Percent(DataPayload<PercentEssentialsV1>),
    /// `currencyDisplay` `symbol`, `narrowSymbol` or `code`: the locale's
    /// currency pattern around the symbol or the ISO code (`$5.00`,
    /// `5,00 €`, `USD 5.00`).
    Currency {
        essentials: DataPayload<CurrencyEssentialsV1>,
        label: CurrencyLabel,
        accounting: bool,
    },
    /// `currencyDisplay: "name"`: the plural currency name (`2.00 US dollars`).
    CurrencyName {
        names: Option<DataPayload<CurrencyExtendedDataV1>>,
        code: TinyAsciiStr<3>,
        patterns: DataPayload<CurrencyPatternsDataV1>,
        plural_rules: PluralRules,
    },
    /// `style: "unit"` with CLDR names for the unit (`3 km`, `3 kilometers`).
    Unit {
        names: DataPayload<ErasedMarker<UnitsDisplayNames<'static>>>,
        plural_rules: PluralRules,
    },
    /// `style: "unit"` for a unit CLDR's categorized data lacks: a fixed
    /// short label (`21°C`, `50 km/h`).
    UnitLabel { label: String, spaced: bool },
}

/// The text a currency pattern places next to the number.
#[derive(Debug)]
#[allow(variant_size_differences)] // a payload pointer beside a 3-byte code
pub(crate) enum CurrencyLabel {
    Symbol(DataPayload<CurrencySymbolsV1>),
    Code(TinyAsciiStr<3>),
}

impl CurrencyLabel {
    fn text(&self) -> &str {
        match self {
            Self::Symbol(symbol) => symbol.get().as_str(),
            Self::Code(code) => code.as_str(),
        }
    }

    /// Whether the label begins and ends with a letter, which makes CLDR
    /// pick the pattern that spaces it from the number (`USD 5.00`, `CHF 5.00`).
    fn letters(&self) -> (bool, bool) {
        match self {
            Self::Symbol(symbol) => (
                symbol.get().starts_with_letter(),
                symbol.get().ends_with_letter(),
            ),
            Self::Code(_) => (true, true),
        }
    }
}

/// ISO 4217's minor-unit digits for `currency`: `2` for USD, `0` for JPY,
/// `3` for KWD. The default `minimumFractionDigits` and
/// `maximumFractionDigits` of a currency format.
pub(crate) fn currency_digits(provider: &IntlProvider, currency: &str) -> u8 {
    let Ok(currency) = CurrencyType::try_from_str(&currency.cow_to_ascii_lowercase()) else {
        return 2;
    };
    DataProvider::<CurrencyFractionsV1>::load(provider, DataRequest::default())
        .map_or(2, |res| res.payload.get().resolve(currency).digits)
}

fn load_with_attributes<M: DataMarker>(
    provider: &(impl DataProvider<M> + ?Sized),
    attributes: &str,
    locale: &DataLocale,
) -> Result<Option<DataResponse<M>>, DataError> {
    let Ok(attributes) = DataMarkerAttributes::try_from_str(attributes) else {
        return Ok(None);
    };
    // A miss is expected (a unit is in one category's table, a currency may
    // lack a symbol), so it is not logged as a data error.
    let mut metadata = DataRequestMetadata::default();
    metadata.silent = true;
    provider
        .load(DataRequest {
            id: DataIdentifierBorrowed::for_marker_attributes_and_locale(attributes, locale),
            metadata,
        })
        .allow_identifier_not_found()
}

impl StyleFormatter {
    /// The style formatter for `options` in `locale`.
    pub(crate) fn try_new(
        provider: &IntlProvider,
        locale: &Locale,
        options: &UnitFormatOptions,
    ) -> Result<Self, DataError> {
        let data_locale = DataLocale::from(locale);
        let for_locale = || DataRequest {
            id: DataIdentifierBorrowed::for_locale(&data_locale),
            ..Default::default()
        };
        Ok(match options {
            UnitFormatOptions::Decimal => Self::Decimal,
            UnitFormatOptions::Percent => Self::Percent(
                DataProvider::<PercentEssentialsV1>::load(provider, for_locale())?.payload,
            ),
            UnitFormatOptions::Currency {
                currency,
                display,
                sign,
            } => {
                let code = TinyAsciiStr::<3>::try_from_str(currency.as_str())
                    .map_err(|_| DataErrorKind::IdentifierNotFound.into_error())?;
                if *display == CurrencyDisplay::Name {
                    let names = load_with_attributes::<CurrencyExtendedDataV1>(
                        provider,
                        code.as_str(),
                        &data_locale,
                    )?
                    .map(|res| res.payload);
                    Self::CurrencyName {
                        names,
                        code,
                        patterns: DataProvider::<CurrencyPatternsDataV1>::load(
                            provider,
                            DataRequest::default(),
                        )?
                        .payload,
                        plural_rules: PluralRules::try_new_cardinal_unstable(
                            provider,
                            locale.into(),
                        )?,
                    }
                } else {
                    let essentials =
                        DataProvider::<CurrencyEssentialsV1>::load(provider, for_locale())?.payload;
                    let label = if *display == CurrencyDisplay::Code {
                        CurrencyLabel::Code(code)
                    } else {
                        let width = if *display == CurrencyDisplay::NarrowSymbol {
                            "n"
                        } else {
                            "s"
                        };
                        load_with_attributes::<CurrencySymbolsV1>(
                            provider,
                            &format!("{width}/{}", code.as_str()),
                            &data_locale,
                        )?
                        .map_or(CurrencyLabel::Code(code), |res| {
                            CurrencyLabel::Symbol(res.payload)
                        })
                    };
                    Self::Currency {
                        essentials,
                        label,
                        accounting: *sign == CurrencySign::Accounting,
                    }
                }
            }
            UnitFormatOptions::Unit { unit, display } => {
                Self::try_new_unit(provider, locale, &data_locale, unit, *display)?
            }
        })
    }

    fn try_new_unit(
        provider: &IntlProvider,
        locale: &Locale,
        data_locale: &DataLocale,
        unit: &Unit,
        display: UnitDisplay,
    ) -> Result<Self, DataError> {
        let id = unit.to_js_string().to_std_string_escaped();
        let width = match display {
            UnitDisplay::Short => "short",
            UnitDisplay::Narrow => "narrow",
            UnitDisplay::Long => "long",
        };
        let attributes = format!("{width}-{id}");

        // A unit's names live in one of a category's tables, but locale
        // fallback reaches the root locale's copy (where `long` is just the
        // short symbol, `km`) in an earlier table before the requested
        // language's entry in a later one. So the first entry in the
        // requested language wins, and a root entry is only the last resort.
        let mut root_names = None;
        macro_rules! try_markers {
            ($($marker:ty),* $(,)?) => {
                $(
                    if let Some(res) =
                        load_with_attributes::<$marker>(provider, &attributes, data_locale)?
                    {
                        let is_root = res
                            .metadata
                            .locale
                            .as_ref()
                            .is_some_and(DataLocale::is_unknown);
                        let names: DataPayload<ErasedMarker<UnitsDisplayNames<'static>>> =
                            res.payload.cast();
                        if !is_root {
                            return Ok(Self::Unit {
                                names,
                                plural_rules: PluralRules::try_new_cardinal_unstable(
                                    provider,
                                    locale.into(),
                                )?,
                            });
                        }
                        root_names.get_or_insert(names);
                    }
                )*
            };
        }
        try_markers!(
            units::UnitsNamesLengthCoreV1,
            units::UnitsNamesLengthExtendedV1,
            units::UnitsNamesMassCoreV1,
            units::UnitsNamesMassExtendedV1,
            units::UnitsNamesVolumeCoreV1,
            units::UnitsNamesVolumeExtendedV1,
            units::UnitsNamesAreaCoreV1,
            units::UnitsNamesAreaExtendedV1,
            units::UnitsNamesDurationCoreV1,
            units::UnitsNamesDurationExtendedV1,
        );
        if let Some(names) = root_names {
            return Ok(Self::Unit {
                names,
                plural_rules: PluralRules::try_new_cardinal_unstable(provider, locale.into())?,
            });
        }

        let (label, spaced) = match id.as_str() {
            "celsius" => ("°C", false),
            "fahrenheit" => ("°F", false),
            "degree" => ("deg", true),
            "percent" => ("%", false),
            "kilometer-per-hour" => ("km/h", true),
            "mile-per-hour" => ("mph", true),
            "meter-per-second" => ("m/s", true),
            "liter-per-kilometer" => ("L/km", true),
            "mile-per-gallon" => ("mpg", true),
            "bit" => ("bit", true),
            "byte" => ("byte", true),
            "kilobit" => ("kb", true),
            "kilobyte" => ("kB", true),
            "megabit" => ("Mb", true),
            "megabyte" => ("MB", true),
            "gigabit" => ("Gb", true),
            "gigabyte" => ("GB", true),
            "terabit" => ("Tb", true),
            "terabyte" => ("TB", true),
            "petabyte" => ("PB", true),
            _ => ("", true),
        };
        let label = if label.is_empty() {
            id
        } else {
            label.to_owned()
        };
        Ok(Self::UnitLabel { label, spaced })
    }

    /// Whether the value is multiplied by 100 before it is rounded.
    pub(crate) fn scales_by_hundred(&self) -> bool {
        matches!(self, Self::Percent(_))
    }

    /// Place the formatted number in the style's pattern. `signed` is the
    /// number as the decimal formatter wrote it, `unsigned` the same number
    /// without its sign, and `value` the rounded decimal (for plural forms).
    pub(crate) fn apply(&self, signed: &str, unsigned: &str, value: &Decimal) -> String {
        let sign = signed.strip_suffix(unsigned).unwrap_or("");
        let negative = value.sign() == Sign::Negative && !sign.is_empty();
        match self {
            Self::Decimal => signed.to_owned(),
            Self::Percent(essentials) => {
                let essentials = essentials.get();
                if sign.is_empty() {
                    essentials
                        .unsigned_pattern
                        .interpolate([unsigned])
                        .write_to_string()
                        .into_owned()
                } else {
                    essentials
                        .signed_pattern
                        .interpolate((unsigned, sign))
                        .write_to_string()
                        .into_owned()
                }
            }
            Self::Currency {
                essentials,
                label,
                accounting,
            } => {
                let essentials = essentials.get();
                let (starts, ends) = label.letters();
                let explicit_negative = if negative {
                    if *accounting {
                        essentials.get_negative_accounting(starts, ends)
                    } else {
                        essentials.get_negative(starts, ends)
                    }
                } else {
                    None
                };
                if let Some(pattern) = explicit_negative {
                    return pattern
                        .interpolate((unsigned, label.text()))
                        .write_to_string()
                        .into_owned();
                }
                let pattern = if *accounting {
                    essentials.get_positive_accounting(starts, ends)
                } else {
                    essentials.get_positive(starts, ends)
                };
                let body = pattern
                    .interpolate((unsigned, label.text()))
                    .write_to_string()
                    .into_owned();
                format!("{sign}{body}")
            }
            Self::CurrencyName {
                names,
                code,
                patterns,
                plural_rules,
            } => {
                let operands = PluralOperands::from(value);
                let name = names.as_ref().map_or(code.as_str(), |names| {
                    names.get().get(operands, plural_rules)
                });
                patterns
                    .get()
                    .get(operands, plural_rules)
                    .interpolate((signed, name))
                    .write_to_string()
                    .into_owned()
            }
            Self::Unit {
                names,
                plural_rules,
            } => names
                .get()
                .get(PluralOperands::from(value), plural_rules)
                .interpolate([signed])
                .write_to_string()
                .into_owned(),
            Self::UnitLabel { label, spaced } => {
                if *spaced {
                    format!("{signed} {label}")
                } else {
                    format!("{signed}{label}")
                }
            }
        }
    }
}
