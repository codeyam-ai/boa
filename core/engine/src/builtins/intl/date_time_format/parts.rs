//! `formatToParts`, `formatRange` and `formatRangeToParts`: a formatted
//! date split into typed pieces, and two dates joined into a range.

use crate::{
    Context, JsArgs, JsResult, JsValue,
    builtins::intl::date_time_format::{DateTimeFormat, zoned_date_time},
    error::JsNativeError,
    js_error, js_string,
    object::ObjectInitializer,
    property::Attribute,
};

/// `x` for the format methods: the current time for `undefined`, else
/// `ToNumber(date)`.
fn date_value(date: &JsValue, context: &mut Context) -> JsResult<f64> {
    if date.is_undefined() {
        Ok(context.clock().system_time_millis() as f64)
    } else {
        date.to_number(context)
    }
}

/// The separator between the two ends of a `formatRange` (` – `, thin
/// spaces around an en dash, as CLDR's interval fallback has it).
const RANGE_SEPARATOR: &str = "\u{2009}\u{2013}\u{2009}";

impl DateTimeFormat {
    /// The `formatToParts` array for `pieces`, each element tagged with
    /// `source` when it is given (`formatRangeToParts`).
    pub(super) fn parts_array(
        pieces: &[(&'static str, String, Option<&'static str>)],
        context: &mut Context,
    ) -> JsValue {
        let elements: Vec<JsValue> = pieces
            .iter()
            .map(|(kind, value, source)| {
                let mut part = ObjectInitializer::new(context);
                part.property(js_string!("type"), js_string!(*kind), Attribute::all())
                    .property(
                        js_string!("value"),
                        js_string!(value.as_str()),
                        Attribute::all(),
                    );
                if let Some(source) = source {
                    part.property(js_string!("source"), js_string!(*source), Attribute::all());
                }
                part.build().into()
            })
            .collect();
        crate::builtins::Array::create_array_from_list(elements, context).into()
    }

    /// [`Intl.DateTimeFormat.prototype.formatToParts ( date )`][spec].
    ///
    /// [spec]: https://tc39.es/ecma402/#sec-Intl.DateTimeFormat.prototype.formatToParts
    pub(super) fn format_to_parts(
        this: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        // 1. Let dtf be the this value.
        // 2. Perform ? RequireInternalSlot(dtf, [[InitializedDateTimeFormat]]).
        let dtf = this
            .as_object()
            .and_then(|o| o.clone().downcast::<DateTimeFormat>().ok())
            .ok_or_else(|| {
                JsNativeError::typ().with_message("`formatToParts` needs an `Intl.DateTimeFormat`")
            })?;
        // 3–4. Let x be the date value.
        let x = date_value(args.get_or_undefined(0), context)?;
        // 5. Return ? FormatDateTimeToParts(dtf, x).
        let pieces: Vec<_> = {
            let dtf = dtf.borrow();
            let dtf = dtf.data();
            let zdt = zoned_date_time(dtf, x, context)?;
            date_time_parts(&dtf.formatter.format(&zdt))
                .into_iter()
                .map(|(kind, value)| (kind, value, None))
                .collect()
        };
        Ok(Self::parts_array(&pieces, context))
    }

    /// The two ends of a range, or one piece list when both format alike.
    pub(super) fn range_pieces(
        this: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Vec<(&'static str, String, Option<&'static str>)>> {
        let dtf = this
            .as_object()
            .and_then(|o| o.clone().downcast::<DateTimeFormat>().ok())
            .ok_or_else(|| {
                JsNativeError::typ().with_message("`formatRange` needs an `Intl.DateTimeFormat`")
            })?;
        let (start, end) = (args.get_or_undefined(0), args.get_or_undefined(1));
        // 3. If startDate is undefined or endDate is undefined, throw a TypeError exception.
        if start.is_undefined() || end.is_undefined() {
            return Err(js_error!(TypeError: "formatRange needs a start and an end date"));
        }
        // 4–5. Let x be ? ToNumber(startDate), y be ? ToNumber(endDate).
        let x = start.to_number(context)?;
        let y = end.to_number(context)?;
        let dtf = dtf.borrow();
        let dtf = dtf.data();
        let from = date_time_parts(&dtf.formatter.format(&zoned_date_time(dtf, x, context)?));
        let to = date_time_parts(&dtf.formatter.format(&zoned_date_time(dtf, y, context)?));
        if from == to {
            return Ok(from
                .into_iter()
                .map(|(k, v)| (k, v, Some("shared")))
                .collect());
        }
        let mut pieces: Vec<_> = from
            .into_iter()
            .map(|(k, v)| (k, v, Some("startRange")))
            .collect();
        pieces.push(("literal", RANGE_SEPARATOR.to_owned(), Some("shared")));
        pieces.extend(to.into_iter().map(|(k, v)| (k, v, Some("endRange"))));
        Ok(pieces)
    }

    /// [`Intl.DateTimeFormat.prototype.formatRange ( startDate, endDate )`][spec].
    ///
    /// [spec]: https://tc39.es/ecma402/#sec-intl.datetimeformat.prototype.formatRange
    pub(super) fn format_range(
        this: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        let text: String = Self::range_pieces(this, args, context)?
            .into_iter()
            .map(|(_, value, _)| value)
            .collect();
        Ok(js_string!(text.as_str()).into())
    }

    /// [`Intl.DateTimeFormat.prototype.formatRangeToParts ( startDate, endDate )`][spec].
    ///
    /// [spec]: https://tc39.es/ecma402/#sec-Intl.DateTimeFormat.prototype.formatRangeToParts
    pub(super) fn format_range_to_parts(
        this: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        let pieces = Self::range_pieces(this, args, context)?;
        Ok(Self::parts_array(&pieces, context))
    }
}

/// The `type` of a `formatToParts` element for an ICU4X date-time part.
fn part_type(part: writeable::Part) -> &'static str {
    use icu_datetime::parts;
    match part {
        p if p == parts::ERA => "era",
        p if p == parts::YEAR || p == parts::EXTENDED_YEAR => "year",
        p if p == parts::RELATED_YEAR => "relatedYear",
        p if p == parts::YEAR_NAME => "yearName",
        p if p == parts::MONTH => "month",
        p if p == parts::DAY => "day",
        p if p == parts::WEEKDAY => "weekday",
        p if p == parts::DAY_PERIOD => "dayPeriod",
        p if p == parts::HOUR => "hour",
        p if p == parts::MINUTE => "minute",
        p if p == parts::SECOND => "second",
        p if p == parts::TIME_ZONE_NAME => "timeZoneName",
        _ => "literal",
    }
}

/// Splits a formatted date into `formatToParts` pieces: each outermost
/// ICU4X part becomes its typed piece, the text between them `literal`.
fn date_time_parts(formatted: &impl writeable::Writeable) -> Vec<(&'static str, String)> {
    struct Collector {
        out: String,
        depth: usize,
        parts: Vec<(usize, usize, writeable::Part)>,
    }
    impl core::fmt::Write for Collector {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            self.out.push_str(s);
            Ok(())
        }
    }
    impl writeable::PartsWrite for Collector {
        type SubPartsWrite = Self;
        fn with_part(
            &mut self,
            part: writeable::Part,
            mut f: impl FnMut(&mut Self) -> core::fmt::Result,
        ) -> core::fmt::Result {
            let start = self.out.len();
            self.depth += 1;
            f(self)?;
            self.depth -= 1;
            if self.depth == 0 {
                self.parts.push((start, self.out.len(), part));
            }
            Ok(())
        }
    }

    let mut collector = Collector {
        out: String::new(),
        depth: 0,
        parts: Vec::new(),
    };
    if formatted.write_to_parts(&mut collector).is_err() {
        return Vec::new();
    }
    let mut pieces: Vec<(&'static str, String)> = Vec::new();
    let mut push = |kind: &'static str, text: &str| {
        if text.is_empty() {
            return;
        }
        match pieces.last_mut() {
            Some((last, value)) if *last == "literal" && kind == "literal" => value.push_str(text),
            _ => pieces.push((kind, text.to_owned())),
        }
    };
    let mut cursor = 0;
    for (start, end, part) in &collector.parts {
        push("literal", &collector.out[cursor..*start]);
        push(part_type(*part), &collector.out[*start..*end]);
        cursor = *end;
    }
    push("literal", &collector.out[cursor..]);
    pieces
}
