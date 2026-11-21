//! Hex for ids (the device id, item ids and blob tags), and the random
//! bytes they and keys are made from.

/// 32 bytes from the system's random source.
pub fn random() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the system random source");
    bytes
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trip() {
        let bytes = [0u8, 1, 0xab, 0xff];
        assert_eq!(super::hex(&bytes), "0001abff");
        assert_eq!(super::unhex("0001abff").unwrap(), bytes);
        assert_eq!(super::unhex("abc"), None);
        assert_eq!(super::unhex("zz"), None);
    }
}
