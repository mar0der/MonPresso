//! Hotkey text such as "Ctrl+Shift+PrintScreen" <-> RegisterHotKey arguments.

pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;
pub const MOD_NOREPEAT: u32 = 0x4000;

const NAMED: &[(&str, u32)] = &[
    ("PrintScreen", 0x2C),
    ("PrtSc", 0x2C),
    ("Pause", 0x13),
    ("Space", 0x20),
    ("Tab", 0x09),
    ("Insert", 0x2D),
    ("Delete", 0x2E),
    ("Home", 0x24),
    ("End", 0x23),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
    ("ScrollLock", 0x91),
];

/// `Ok(None)` for an empty (disabled) hotkey.
pub fn parse(text: &str) -> Result<Option<(u32, u32)>, String> {
    let text = text.trim();
    if text.is_empty() || text.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    let mut mods = 0;
    let mut key = None;
    for part in text.split('+').map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods |= MOD_CONTROL,
            "alt" => mods |= MOD_ALT,
            "shift" => mods |= MOD_SHIFT,
            "win" | "windows" => mods |= MOD_WIN,
            _ => {
                if key.is_some() {
                    return Err(format!("'{text}' has more than one key"));
                }
                key = Some(vk(part).ok_or_else(|| format!("Unknown key '{part}' in '{text}'"))?);
            }
        }
    }
    key.map(|k| Some((mods, k))).ok_or_else(|| format!("'{text}' has no key"))
}

fn vk(name: &str) -> Option<u32> {
    if let Some((_, v)) = NAMED.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
        return Some(*v);
    }
    let upper = name.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    if bytes.len() == 1 && (bytes[0].is_ascii_uppercase() || bytes[0].is_ascii_digit()) {
        return Some(bytes[0] as u32);
    }
    if let Some(n) = upper.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) {
            return Some(0x6F + n);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hotkeys() {
        assert_eq!(parse("PrintScreen").unwrap(), Some((0, 0x2C)));
        assert_eq!(parse("Ctrl+Shift+PrintScreen").unwrap(), Some((MOD_CONTROL | MOD_SHIFT, 0x2C)));
        assert_eq!(parse("alt+f5").unwrap(), Some((MOD_ALT, 0x74)));
        assert_eq!(parse("Win+S").unwrap(), Some((MOD_WIN, b'S' as u32)));
        assert_eq!(parse("").unwrap(), None);
        assert!(parse("Ctrl+Nope").is_err());
        assert!(parse("Ctrl").is_err());
    }
}
