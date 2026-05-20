//! Display helpers shared across CLI subcommands.
//!
//! Plain `u128` planck values are unreadable past a few zeros — every operator
//! command prints money figures (bonds, fees, funding amounts), so go through
//! [`plancks`] to render them with the chain's 12-decimal scaling. Trailing
//! zeros in the fractional part are trimmed so common round numbers stay
//! short ("10" instead of "10.000000000000").

/// Allfeat-runtime balance scaling: `1 token = 10^12 plancks`. Hard-coded
/// because every supported runtime ships with the same value and pulling it
/// from the chain would force every CLI helper that displays an amount to
/// become async.
const DECIMALS: u32 = 12;

/// Format a planck amount with the chain's 12 decimals, trimming trailing
/// zeros from the fractional portion.
///
/// Examples:
/// - `0` → `"0"`
/// - `10_000_000_000_000` → `"10"` (1 token, no fractional part)
/// - `100_000_000` → `"0.0001"`
/// - `261_415_440_000_000` → `"261.41544"`
pub fn plancks(amount: u128) -> String {
    let divisor = 10_u128.pow(DECIMALS);
    let whole = amount / divisor;
    let frac = amount % divisor;
    if frac == 0 {
        return whole.to_string();
    }
    let frac_str = format!("{frac:0>width$}", width = DECIMALS as usize);
    let trimmed = frac_str.trim_end_matches('0');
    format!("{whole}.{trimmed}")
}

/// Lowercase hex of a SCALE-encoded payload, no `0x` prefix. The `create`
/// wizard emits this as the wire-ready blob; a tiny hand-rolled encoder keeps
/// the CLI free of a `hex` dependency for what is two lines of formatting.
pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero() {
        assert_eq!(plancks(0), "0");
    }

    #[test]
    fn whole_token() {
        assert_eq!(plancks(10_000_000_000_000), "10");
    }

    #[test]
    fn small_fraction() {
        assert_eq!(plancks(100_000_000), "0.0001");
    }

    #[test]
    fn mixed() {
        assert_eq!(plancks(261_415_440_000_000), "261.41544");
    }

    #[test]
    fn very_small() {
        assert_eq!(plancks(1), "0.000000000001");
    }

    #[test]
    fn hex_empty() {
        assert_eq!(hex(&[]), "");
    }

    #[test]
    fn hex_pads_each_byte_to_two_nibbles() {
        assert_eq!(hex(&[0x00, 0x0a, 0xff, 0x10]), "000aff10");
    }
}
