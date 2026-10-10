//! JSON number readers shared by providers.

use serde_json::Value;

/// A JSON number, or a string holding one once trimmed and stripped of
/// thousands commas. Strings that parse to a non-finite value pass through.
pub(crate) fn lenient_f64(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().replace(',', "").parse().ok(),
        _ => None,
    }
}

/// [`lenient_f64`], keeping finite values only.
pub(crate) fn lenient_finite_f64(value: &Value) -> Option<f64> {
    lenient_f64(Some(value)).filter(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lenient_f64_reads_numbers_and_numeric_strings() {
        assert_eq!(lenient_f64(Some(&json!(12.5))), Some(12.5));
        assert_eq!(lenient_f64(Some(&json!(" 1,234.5 "))), Some(1234.5));
        assert_eq!(lenient_f64(Some(&json!("abc"))), None);
        assert_eq!(lenient_f64(Some(&json!(true))), None);
        assert_eq!(lenient_f64(None), None);
        assert!(lenient_f64(Some(&json!("NaN"))).is_some_and(f64::is_nan));
    }

    #[test]
    fn lenient_finite_f64_drops_non_finite_strings() {
        assert_eq!(lenient_finite_f64(&json!("7")), Some(7.0));
        assert_eq!(lenient_finite_f64(&json!("inf")), None);
        assert_eq!(lenient_finite_f64(&json!("NaN")), None);
    }
}
