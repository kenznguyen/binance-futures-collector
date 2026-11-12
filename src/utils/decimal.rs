use anyhow::{bail, Result};

/// Normalize a plain exchange decimal into a canonical key without changing its value.
/// Exponent notation is rejected here because Binance order-book price/quantity levels
/// are expected as ordinary decimal strings.
pub fn normalize_plain_decimal(value: &str) -> Result<String> {
    let (negative, unsigned) = match value.as_bytes().first() {
        Some(b'-') => (true, &value[1..]),
        Some(b'+') => (false, &value[1..]),
        _ => (false, value),
    };
    if unsigned.is_empty() || unsigned.bytes().any(|byte| byte == b'e' || byte == b'E') {
        bail!("invalid plain decimal value: {value}");
    }

    let mut parts = unsigned.split('.');
    let integer = parts.next().unwrap_or_default();
    let fraction = parts.next().unwrap_or_default();
    if parts.next().is_some()
        || (integer.is_empty() && fraction.is_empty())
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        bail!("invalid plain decimal value: {value}");
    }

    let integer = integer.trim_start_matches('0');
    let integer = if integer.is_empty() { "0" } else { integer };
    let fraction = fraction.trim_end_matches('0');
    let is_zero = integer == "0" && fraction.is_empty();

    let mut normalized = String::new();
    if negative && !is_zero {
        normalized.push('-');
    }
    normalized.push_str(integer);
    if !fraction.is_empty() {
        normalized.push('.');
        normalized.push_str(fraction);
    }
    Ok(normalized)
}

/// Compare two values returned by normalize_plain_decimal.
pub fn compare_normalized_decimals(left: &str, right: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let (left_negative, left) = left.strip_prefix('-').map_or((false, left), |v| (true, v));
    let (right_negative, right) = right
        .strip_prefix('-')
        .map_or((false, right), |v| (true, v));
    if left_negative != right_negative {
        return if left_negative {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }

    let (left_integer, left_fraction) = left.split_once('.').unwrap_or((left, ""));
    let (right_integer, right_fraction) = right.split_once('.').unwrap_or((right, ""));
    let magnitude = left_integer
        .len()
        .cmp(&right_integer.len())
        .then_with(|| left_integer.cmp(right_integer))
        .then_with(|| {
            let max_len = left_fraction.len().max(right_fraction.len());
            for index in 0..max_len {
                let a = left_fraction.as_bytes().get(index).copied().unwrap_or(b'0');
                let b = right_fraction
                    .as_bytes()
                    .get(index)
                    .copied()
                    .unwrap_or(b'0');
                match a.cmp(&b) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
            Ordering::Equal
        });

    if left_negative {
        magnitude.reverse()
    } else {
        magnitude
    }
}

/// Multiply two exchange decimal strings without converting through binary floating point.
/// The result is exact for the supplied finite decimal values and retains their combined scale.
pub fn multiply_decimal_strings(left: &str, right: &str) -> Result<String> {
    fn parse(value: &str) -> Result<(bool, Vec<u8>, usize)> {
        let (mantissa, exponent) =
            match value.find(|character| character == 'e' || character == 'E') {
                Some(index) => {
                    let exponent = value[index + 1..].parse::<i32>()?;
                    (&value[..index], exponent)
                }
                None => (value, 0),
            };

        let (negative, unsigned) = match mantissa.as_bytes().first() {
            Some(b'-') => (true, &mantissa[1..]),
            Some(b'+') => (false, &mantissa[1..]),
            _ => (false, mantissa),
        };
        if unsigned.is_empty() {
            bail!("empty decimal mantissa");
        }

        let mut parts = unsigned.split('.');
        let integer = parts.next().unwrap_or_default();
        let fraction = parts.next().unwrap_or_default();
        if parts.next().is_some()
            || (integer.is_empty() && fraction.is_empty())
            || !integer.bytes().all(|b| b.is_ascii_digit())
            || !fraction.bytes().all(|b| b.is_ascii_digit())
        {
            bail!("invalid decimal value: {value}");
        }

        let mut digits = format!("{integer}{fraction}");
        let first_nonzero = digits.find(|c: char| c != '0').unwrap_or(digits.len());
        digits.drain(..first_nonzero);
        if digits.is_empty() {
            return Ok((false, vec![0], 0));
        }

        let mut scale = fraction.len() as i64 - i64::from(exponent);
        if scale < 0 {
            let zeros = usize::try_from(-scale)?;
            if zeros > 10_000 {
                bail!("decimal exponent out of supported bounds");
            }
            digits.extend(std::iter::repeat_n('0', zeros));
            scale = 0;
        }
        if scale > 10_000 || digits.len() > 10_000 {
            bail!("decimal value exceeds supported precision bounds");
        }

        let digits = digits.bytes().map(|b| b - b'0').collect::<Vec<_>>();
        Ok((negative, digits, scale as usize))
    }

    let (left_negative, left_digits, left_scale) = parse(left)?;
    let (right_negative, right_digits, right_scale) = parse(right)?;
    let mut product = vec![0u32; left_digits.len() + right_digits.len()];

    for (left_index, left_digit) in left_digits.iter().rev().enumerate() {
        for (right_index, right_digit) in right_digits.iter().rev().enumerate() {
            product[left_index + right_index] += u32::from(*left_digit) * u32::from(*right_digit);
        }
    }
    let mut index = 0;
    while index < product.len() {
        let carry = product[index] / 10;
        product[index] %= 10;
        if carry > 0 {
            if index + 1 == product.len() {
                product.push(carry);
            } else {
                product[index + 1] += carry;
            }
        }
        index += 1;
    }
    while product.len() > 1 && product.last() == Some(&0) {
        product.pop();
    }

    let mut digits = product
        .iter()
        .rev()
        .map(|digit| char::from(b'0' + (*digit as u8)))
        .collect::<String>();
    let scale = left_scale + right_scale;
    if scale > 10_000 {
        bail!("decimal product exceeds supported scale");
    }
    if digits.len() <= scale {
        digits = format!("{}{}", "0".repeat(scale + 1 - digits.len()), digits);
    }
    if scale > 0 {
        digits.insert(digits.len() - scale, '.');
    }
    if left_negative ^ right_negative && digits.bytes().any(|b| b.is_ascii_digit() && b != b'0') {
        digits.insert(0, '-');
    }
    Ok(digits)
}

#[cfg(test)]
mod tests {
    use super::multiply_decimal_strings;

    #[test]
    fn normalization_and_comparison_are_exact_for_order_book_prices() {
        use super::{compare_normalized_decimals, normalize_plain_decimal};
        use std::cmp::Ordering;

        let two = normalize_plain_decimal("2.000").unwrap();
        let ten = normalize_plain_decimal("10").unwrap();
        let one = normalize_plain_decimal("1.00").unwrap();
        let one_plain = normalize_plain_decimal("1").unwrap();
        let negative_two = normalize_plain_decimal("-2.0").unwrap();
        let negative_one = normalize_plain_decimal("-1").unwrap();
        let tiny = normalize_plain_decimal("0.0002").unwrap();
        let small = normalize_plain_decimal("0.001").unwrap();

        assert_eq!(compare_normalized_decimals(&two, &ten), Ordering::Less);
        assert_eq!(
            compare_normalized_decimals(&one, &one_plain),
            Ordering::Equal
        );
        assert_eq!(
            compare_normalized_decimals(&negative_two, &negative_one),
            Ordering::Less
        );
        assert_eq!(compare_normalized_decimals(&tiny, &small), Ordering::Less);
    }

    #[test]
    fn order_book_normalization_rejects_malformed_and_exponent_forms() {
        assert!(super::normalize_plain_decimal("1e-8").is_err());
        assert!(super::normalize_plain_decimal("not-a-price").is_err());
        assert!(super::normalize_plain_decimal("1.2.3").is_err());
    }

    #[test]
    fn multiplication_preserves_all_source_precision() {
        assert_eq!(
            multiply_decimal_strings("62456.601234567891234567", "0.00000001").unwrap(),
            "0.00062456601234567891234567"
        );
        assert_eq!(
            multiply_decimal_strings("123.4500", "0.0060").unwrap(),
            "0.74070000"
        );
        assert_eq!(multiply_decimal_strings("-12.5", "2e-2").unwrap(), "-0.250");
    }

    #[test]
    fn multiplication_rejects_malformed_values() {
        assert!(multiply_decimal_strings("not-a-number", "2").is_err());
        assert!(multiply_decimal_strings("1.0.0", "2").is_err());
        assert!(multiply_decimal_strings("1", "NaN").is_err());
    }
}
