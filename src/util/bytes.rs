use std::fmt;

pub(crate) struct Bytes(pub u64);

impl fmt::Display for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
        let mut value = self.0 as f64;
        let mut unit = 0;
        while value >= 1023.5 && unit < UNITS.len() - 1 {
            value /= 1024.0;
            unit += 1;
        }
        let unit = UNITS[unit];
        if unit == "B" {
            write!(f, "{} B", self.0)
        } else if value < 9.995 {
            write!(f, "{value:.2} {unit}")
        } else if value < 99.95 {
            write!(f, "{value:.1} {unit}")
        } else {
            write!(f, "{value:.0} {unit}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Bytes;

    #[test]
    fn three_figures_across_units() {
        let s = |n: u64| Bytes(n).to_string();
        assert_eq!(s(0), "0 B");
        assert_eq!(s(1023), "1023 B");
        assert_eq!(s(1024), "1.00 KiB");
        assert_eq!(s(1434), "1.40 KiB");
        assert_eq!(s(10_235), "10.0 KiB");
        assert_eq!(s(1024 * 1024 - 1), "1.00 MiB");
        assert_eq!(s(2_202_009), "2.10 MiB");
        assert_eq!(s(512 << 20), "512 MiB");
        assert_eq!(s(5 << 40), "5120 GiB");
    }
}
