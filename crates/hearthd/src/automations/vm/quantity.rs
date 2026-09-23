//! [`Quantity`]: unit literals in their dimension's canonical unit.

use crate::automations::lexer::UnitType;

/// A magnitude in its dimension's canonical unit.
///
/// The variant carries the unit, so a magnitude cannot be paired with the
/// wrong one. The canonical units mirror Matter's own encodings wherever
/// Matter has one — hundredths of a degree Celsius, tenths of a degree —
/// so projecting cluster state is a retag rather than a conversion. Matter
/// has no single duration encoding (its attributes variously use seconds,
/// milliseconds and epoch microseconds), so durations take nanoseconds,
/// finer than any of them and still spanning 292 years.
///
/// The width is ours rather than Matter's: an `i16` measurement widens into
/// an `i64` for free, and a language value should not be capped by the wire
/// format it interoperates with — `1000c` and `3600deg` are legitimate
/// literals that Matter's own widths would reject. All three are signed,
/// which temperature needs outright, since canonical is degrees *Celsius*
/// and absolute zero is `-27315`.
///
/// Integer rather than floating point, so equality is exact and needs no
/// tolerance: every literal is converted once, on the way in, and two
/// spellings of one value land on the same integer or on different ones.
///
/// The `strum` string is the suffix `Display` renders in, which is the unit
/// an author writes rather than the canonical one; the two differ by a
/// power of ten, so rendering is exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
pub enum Quantity {
    /// Nanoseconds.
    #[strum(serialize = "s")]
    Duration(i64),
    /// Tenths of a degree.
    #[strum(serialize = "deg")]
    Angle(i64),
    /// Hundredths of a degree Celsius.
    #[strum(serialize = "c")]
    Temperature(i64),
}

impl Quantity {
    /// The canonical magnitude, whichever unit this variant holds it in.
    fn magnitude(self) -> i64 {
        match self {
            Quantity::Duration(n) | Quantity::Angle(n) | Quantity::Temperature(n) => n,
        }
    }

    /// Decimal places between the canonical magnitude and the unit
    /// `Display` renders in: nanoseconds to seconds, tenths of a degree to
    /// degrees, hundredths of a degree to degrees Celsius.
    const fn display_decimals(self) -> u32 {
        match self {
            Quantity::Duration(_) => 9,
            Quantity::Angle(_) => 1,
            Quantity::Temperature(_) => 2,
        }
    }
}

impl std::fmt::Display for Quantity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}{}",
            render_scaled(self.magnitude(), self.display_decimals()),
            <&'static str>::from(self)
        )
    }
}

/// Render `value / 10^decimals` exactly, trimming trailing fractional
/// zeros. Exact because the scale is a power of ten.
fn render_scaled(value: i64, decimals: u32) -> String {
    let divisor = 10i64.pow(decimals);
    let sign = if value < 0 { "-" } else { "" };
    // `unsigned_abs` rather than `abs`, so `i64::MIN` does not overflow.
    let magnitude = value.unsigned_abs();
    let whole = magnitude / divisor as u64;
    let frac = magnitude % divisor as u64;
    if frac == 0 {
        return format!("{}{}", sign, whole);
    }
    let frac = format!("{:0width$}", frac, width = decimals as usize);
    format!("{}{}.{}", sign, whole, frac.trim_end_matches('0'))
}

/// How one unit converts into its dimension's canonical magnitude.
///
/// Where the literal is `mantissa / 10^exp`:
///
/// ```text
/// canonical = (mantissa - src_offset * 10^exp) * num / (den * 10^exp) + dst_offset
/// ```
///
/// Subtracting the source-side offset before scaling keeps fahrenheit exact
/// until its one unavoidable division; kelvin's offset is a whole number of
/// canonical units, so it applies on the far side instead.
struct Conversion {
    /// Builds the quantity once the magnitude is in canonical units.
    make: fn(i64) -> Quantity,
    src_offset: i128,
    num: i128,
    den: i128,
    dst_offset: i128,
}

/// Absolute zero in hundredths of a degree Celsius.
const ABSOLUTE_ZERO_CENTI_CELSIUS: i128 = -27315;
/// Where the fahrenheit scale's zero sits relative to freezing.
const FAHRENHEIT_FREEZING: i128 = 32;

impl Quantity {
    /// Convert a unit literal to its dimension's canonical magnitude.
    ///
    /// Takes the literal's decimal text rather than an `f64`: scaling the
    /// decimal directly is what keeps the conversion exact. Going through
    /// `f64` first would not — an `f64` holds integers exactly only to
    /// 2^53, which is about 104 days in nanoseconds, so `200d` would be
    /// wrong before any operation ran.
    ///
    /// Returns `None` if the text does not parse or the result leaves
    /// `i64`. Eight of the nine conversions are exact; fahrenheit divides
    /// by nine and radians scales by 1800/π, so both round to the nearest
    /// canonical unit.
    pub(super) fn from_unit_literal(unit: UnitType, value: &str) -> Option<Quantity> {
        if unit == UnitType::Radians {
            // The only irrational factor, so the only one that cannot be
            // done in integers.
            let degrees = value.parse::<f64>().ok()? * 180.0 / std::f64::consts::PI;
            let tenths = (degrees * 10.0).round();
            if !tenths.is_finite() || tenths.abs() > i64::MAX as f64 {
                return None;
            }
            return Some(Quantity::Angle(tenths as i64));
        }

        let c = Conversion::from(unit);
        let (mantissa, exp) = parse_decimal(value)?;
        let scale = 10i128.checked_pow(exp)?;

        let numerator = mantissa
            .checked_sub(c.src_offset.checked_mul(scale)?)?
            .checked_mul(c.num)?;
        let denominator = c.den.checked_mul(scale)?;
        let scaled = div_round(numerator, denominator)?.checked_add(c.dst_offset)?;

        Some((c.make)(i64::try_from(scaled).ok()?))
    }
}

impl From<UnitType> for Conversion {
    fn from(unit: UnitType) -> Conversion {
        let (make, src_offset, num, den, dst_offset): (
            fn(i64) -> Quantity,
            i128,
            i128,
            i128,
            i128,
        ) = match unit {
            // Durations canonicalise to nanoseconds.
            UnitType::Seconds => (Quantity::Duration, 0, 1_000_000_000, 1, 0),
            UnitType::Minutes => (Quantity::Duration, 0, 60_000_000_000, 1, 0),
            UnitType::Hours => (Quantity::Duration, 0, 3_600_000_000_000, 1, 0),
            UnitType::Days => (Quantity::Duration, 0, 86_400_000_000_000, 1, 0),

            // Angles canonicalise to tenths of a degree. Radians never
            // reach here.
            UnitType::Degrees | UnitType::Radians => (Quantity::Angle, 0, 10, 1, 0),

            // Temperatures canonicalise to hundredths of a degree Celsius.
            UnitType::Celsius => (Quantity::Temperature, 0, 100, 1, 0),
            UnitType::Fahrenheit => (Quantity::Temperature, FAHRENHEIT_FREEZING, 500, 9, 0),
            UnitType::Kelvin => (
                Quantity::Temperature,
                0,
                100,
                1,
                ABSOLUTE_ZERO_CENTI_CELSIUS,
            ),
        };
        Conversion {
            make,
            src_offset,
            num,
            den,
            dst_offset,
        }
    }
}

/// Split decimal text into a mantissa and the power of ten it is divided
/// by, so `"1.5"` becomes `(15, 1)`. Rejects anything the lexer should not
/// have produced.
fn parse_decimal(text: &str) -> Option<(i128, u32)> {
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => (-1i128, rest),
        None => (1i128, text),
    };
    let (whole, frac) = match digits.split_once('.') {
        Some((w, f)) => (w, f),
        None => (digits, ""),
    };
    if whole.is_empty() && frac.is_empty() {
        return None;
    }
    if !whole
        .bytes()
        .chain(frac.bytes())
        .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let mut mantissa: i128 = 0;
    for byte in whole.bytes().chain(frac.bytes()) {
        mantissa = mantissa
            .checked_mul(10)?
            .checked_add(i128::from(byte - b'0'))?;
    }
    Some((sign * mantissa, u32::try_from(frac.len()).ok()?))
}

/// Integer division rounding halves away from zero, so a lossy conversion
/// lands on the nearest canonical unit rather than truncating toward it.
fn div_round(numerator: i128, denominator: i128) -> Option<i128> {
    if denominator == 0 {
        return None;
    }
    let quotient = numerator.checked_div(denominator)?;
    let remainder = numerator.checked_rem(denominator)?;
    if remainder == 0 {
        return Some(quotient);
    }
    let rounds_away = remainder.unsigned_abs() * 2 >= denominator.unsigned_abs();
    if !rounds_away {
        return Some(quotient);
    }
    let step = if (numerator < 0) == (denominator < 0) {
        1
    } else {
        -1
    };
    quotient.checked_add(step)
}
