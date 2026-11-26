//! printf's float formats, for files the C++ half wrote and still reads (and the reverse).

/// printf's `%.9g` of a float (promoted to double): nine significant digits, trailing zeros dropped, the exponent
/// form below 1e-4 and from 1e9 up.
pub fn format_g9(v: f32) -> String {
    format_g(v as f64, 9)
}

/// printf's `%.<p>g` (`%g` is p = 6).
pub fn format_g(v: f64, p: i32) -> String {
    let p = if p == 0 { 1 } else { p };
    if v.is_nan() {
        return if v.is_sign_negative() { "-nan(ind)".into() } else { "nan".into() };
    }
    if v.is_infinite() {
        return if v < 0.0 { "-inf".into() } else { "inf".into() };
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let sci = format!("{:.*e}", (p - 1) as usize, v); // d.dddde<exp> to p significant digits, correctly rounded
    let (mantissa, exp) = sci.split_once('e').unwrap();
    let x: i32 = exp.parse().unwrap();
    let strip = |s: &str| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s.to_string()
        }
    };
    if x < -4 || x >= p {
        let m = strip(mantissa);
        let sign = if x < 0 { '-' } else { '+' };
        format!("{m}e{sign}{:02}", x.abs())
    } else {
        strip(&format!("{:.*}", (p - 1 - x) as usize, v))
    }
}

#[cfg(test)]
mod tests {
    use super::{format_g, format_g9};
    #[test]
    fn printf_plain_g() {
        assert_eq!(format_g(-1.0, 6), "-1");
        assert_eq!(format_g(0.0, 6), "0");
        assert_eq!(format_g(0.1234567, 6), "0.123457");
        assert_eq!(format_g(1234567.0, 6), "1.23457e+06");
    }
    #[test]
    fn printf_g() {
        assert_eq!(format_g9(0.5), "0.5");
        assert_eq!(format_g9(-0.70710677), "-0.707106769");
        assert_eq!(format_g9(1.0), "1");
        assert_eq!(format_g9(2.15937559e-05), "2.15937562e-05"); // the float nearest, to nine digits (as Python's %.9g)
        assert_eq!(format_g9(123456792.0), "123456792");
        assert_eq!(format_g9(1.0e9), "1e+09");
        assert_eq!(format_g9(0.0001), "9.99999975e-05");
    }
}
