use anyhow::{bail, Result};

/// Multiply two exchange decimal strings without converting through binary floating point.
/// The result is exact for the supplied finite decimal values and retains their combined scale.
pub fn multiply_decimal_strings(left: &str, right: &str) -> Result<String> {
    fn parse(value: &str) -> Result<(bool, Vec<u8>, usize)> {
        let (mantissa, exponent) = match value.find(['e', 'E']) {
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

        let digits = digits
            .bytes()
            .map(|b| b - b'0')
            .collect::<Vec<_>>();
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
    for index in 0..product.len() - 1 {
        let carry = product[index] / 10;
        product[index] %= 10;
        product[index + 1] += carry;
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
    fn multiplication_preserves_all_source_precision() {
        assert_eq!(
            multiply_decimal_strings("62456.601234567891234567", "0.00000001").unwrap(),
            "0.00062456601234567891234567"
        );
        assert_eq!(
            multiply_decimal_strings("123.4500", "0.0060").unwrap(),
            "0.74070000"
        );
        assert_eq!(
            multiply_decimal_strings("-12.5", "2e-2").unwrap(),
            "-0.250"
        );
    }

    #[test]
    fn multiplication_rejects_malformed_values() {
        assert!(multiply_decimal_strings("not-a-number", "2").is_err());
        assert!(multiply_decimal_strings("1.0.0", "2").is_err());
        assert!(multiply_decimal_strings("1", "NaN").is_err());
    }
}
