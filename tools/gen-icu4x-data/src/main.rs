#![allow(missing_docs, rustdoc::missing_crate_level_docs)]

use std::path::Path;
use std::{error::Error, fs::File};

use icu_provider_export::blob_exporter::BlobExporter;
use icu_provider_export::prelude::*;
use icu_provider_source::{CoverageLevel, SourceDataProvider};

/// Path to the directory where the exported data lives.
const EXPORT_PATH: &str = "core/icu_provider/data";

/// The `icu_experimental` markers behind `Intl.NumberFormat`'s currency,
/// percent and unit styles and `Intl.DisplayNames`.
const EXPERIMENTAL_MARKERS: &[DataMarkerInfo] = {
    use icu_experimental::dimension::provider::{currency, percent, units};
    use icu_experimental::displaynames::provider as dn;
    use icu_provider::DataMarker;
    use units::categorized_display_names as cdn;
    &[
        currency::displayname::CurrencyDisplaynameV1::INFO,
        currency::essentials::CurrencyEssentialsV1::INFO,
        currency::symbols::CurrencySymbolsV1::INFO,
        currency::patterns::CurrencyPatternsDataV1::INFO,
        currency::extended::CurrencyExtendedDataV1::INFO,
        currency::fractions::CurrencyFractionsV1::INFO,
        currency::no_currency::CurrencyPatternsNoCurrencyV1::INFO,
        percent::PercentEssentialsV1::INFO,
        units::essentials::UnitsEssentialsV1::INFO,
        cdn::UnitsNamesAreaCoreV1::INFO,
        cdn::UnitsNamesAreaExtendedV1::INFO,
        cdn::UnitsNamesDurationCoreV1::INFO,
        cdn::UnitsNamesDurationExtendedV1::INFO,
        cdn::UnitsNamesLengthCoreV1::INFO,
        cdn::UnitsNamesLengthExtendedV1::INFO,
        cdn::UnitsNamesMassCoreV1::INFO,
        cdn::UnitsNamesMassExtendedV1::INFO,
        cdn::UnitsNamesVolumeCoreV1::INFO,
        cdn::UnitsNamesVolumeExtendedV1::INFO,
        dn::LanguageDisplayNamesV1::INFO,
        dn::LocaleDisplayNamesV1::INFO,
        dn::VariantDisplayNamesV1::INFO,
        dn::RegionDisplayNamesV1::INFO,
        dn::ScriptDisplayNamesV1::INFO,
    ]
};

/// List of services used by `Intl` components.
///
/// This must be kept in sync with the list of implemented services for `Intl`.
const SERVICES: &[(&str, &[DataMarkerInfo])] = &[
    ("icu_casemap", icu_casemap::provider::MARKERS),
    ("icu_collator", icu_collator::provider::MARKERS),
    ("icu_datetime", icu_datetime::provider::MARKERS),
    ("icu_time", icu_time::provider::MARKERS),
    ("icu_decimal", icu_decimal::provider::MARKERS),
    ("icu_list", icu_list::provider::MARKERS),
    ("icu_locale", icu_locale::provider::MARKERS),
    ("icu_normalizer", icu_normalizer::provider::MARKERS),
    ("icu_plurals", icu_plurals::provider::MARKERS),
    ("icu_segmenter", icu_segmenter::provider::MARKERS),
    ("icu_calendar", icu_calendar::provider::MARKERS),
    ("icu_experimental", EXPERIMENTAL_MARKERS),
    (
        "icu_locale_fallback",
        icu_locale_fallback::provider::MARKERS,
    ),
];

fn export_for_service(
    service: &str,
    markers: &[DataMarkerInfo],
    provider: &SourceDataProvider,
    driver: ExportDriver,
) -> Result<(), Box<dyn Error>> {
    log::info!("Generating ICU4X data for service `{service}` with markers: {markers:#?}");

    let export_path = Path::new(EXPORT_PATH);
    let export_file = export_path.join(format!("{service}.postcard"));

    driver.with_markers(markers.iter().copied()).export(
        provider,
        BlobExporter::new_with_sink(Box::new(File::create(export_file)?)),
    )?;

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    simple_logger::SimpleLogger::new()
        .env()
        .with_level(log::LevelFilter::Info)
        .init()?;

    // `BOA_ICU4X_SERVICES=icu_experimental,icu_decimal` regenerates only
    // the named services and leaves every other exported blob untouched.
    let only: Option<Vec<String>> = std::env::var("BOA_ICU4X_SERVICES")
        .ok()
        .map(|list| list.split(',').map(|s| s.trim().to_owned()).collect());

    if only.is_none() {
        // Removal will throw an error if the directory doesn't exist, hence
        // why we can ignore the error.
        let _unused = std::fs::remove_dir_all(EXPORT_PATH);
    }
    std::fs::create_dir_all(EXPORT_PATH)?;

    let provider = &SourceDataProvider::new();
    let locales = provider
        .locales_for_coverage_levels([CoverageLevel::Modern])?
        .into_iter()
        .map(DataLocaleFamily::with_descendants)
        .chain([
            // test262 assumes the en-US locale does not fallback.
            // Required by https://github.com/tc39/test262/blob/a073f479f80b336256b7fc4e04700c827293e2fe/test/intl402/ListFormat/prototype/resolvedOptions/type.js
            DataLocaleFamily::single(locale!("en-US").into()),
            // test262 uses the Manx locale.
            // Required by https://github.com/tc39/test262/blob/a073f479f80b336256b7fc4e04700c827293e2fe/test/intl402/PluralRules/prototype/resolvedOptions/plural-categories-order.js
            DataLocaleFamily::with_descendants(locale!("gv").into()),
        ]);

    let driver = ExportDriver::new(
        locales,
        DeduplicationStrategy::None.into(),
        LocaleFallbacker::try_new_unstable(provider)?,
    )
    .with_additional_collations([String::from("search*")])
    .with_recommended_segmenter_models();

    for (service, keys) in SERVICES {
        if only
            .as_ref()
            .is_some_and(|only| !only.iter().any(|s| s == service))
        {
            continue;
        }
        export_for_service(service, keys, provider, driver.clone())?;
    }

    Ok(())
}
