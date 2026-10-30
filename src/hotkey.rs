use anyhow::{Result, anyhow, bail};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Hotkey {
    raw: String,
    sequence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modifier {
    Shift,
    Ctrl,
    Alt,
    Meta,
}

impl Modifier {
    fn parse(token: &str) -> Option<Self> {
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Some(Self::Ctrl),
            "shift" => Some(Self::Shift),
            "alt" => Some(Self::Alt),
            "super" | "meta" | "win" => Some(Self::Meta),
            _ => None,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Shift => "Shift",
            Self::Ctrl => "Ctrl",
            Self::Alt => "Alt",
            Self::Meta => "Meta",
        }
    }

    /// Whether a tap of this modifier on its own may be a hotkey. Ctrl and Shift are held
    /// alone too often, around clicks, drags and input method switches, and `KWin` fires a
    /// modifier tap on any such release.
    const fn taps_alone(self) -> bool {
        matches!(self, Self::Alt | Self::Meta)
    }
}

impl Hotkey {
    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn sequence(&self) -> &str {
        &self.sequence
    }

    pub fn parse(input: &str) -> Result<Self> {
        let raw = input.trim().to_string();
        if raw.is_empty() {
            bail!("hotkey must not be empty");
        }

        let tokens: Vec<_> = raw
            .split('+')
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .collect();

        let Some((key, modifier_tokens)) = tokens.split_last() else {
            bail!("hotkey '{raw}' has no key");
        };

        let mut modifiers = Vec::new();
        for token in modifier_tokens {
            let Some(modifier) = Modifier::parse(token) else {
                bail!(
                    "unknown hotkey modifier '{}' in '{raw}'",
                    token.to_ascii_lowercase()
                );
            };
            if modifiers.contains(&modifier) {
                bail!("duplicate hotkey modifier '{token}' in '{raw}'");
            }
            modifiers.push(modifier);
        }

        if let Some(modifier) = Modifier::parse(key) {
            if !modifiers.is_empty() {
                bail!(
                    "hotkey '{raw}' has no key; a modifier-only hotkey must be a single modifier"
                );
            }
            if !modifier.taps_alone() {
                bail!("hotkey '{raw}' can't be used alone; only super and alt can");
            }
            return Ok(Self {
                sequence: modifier.name().to_string(),
                raw,
            });
        }

        let key = normalize_key(key, &raw)?;
        let sequence = modifiers
            .iter()
            .map(|modifier| modifier.name())
            .chain([key.as_str()])
            .collect::<Vec<_>>()
            .join("+");

        Ok(Self { raw, sequence })
    }
}

fn normalize_key(token: &str, raw: &str) -> Result<String> {
    let lower = token.to_ascii_lowercase();
    let key = match lower.as_str() {
        "grave" | "backtick" | "`" => "`".to_string(),
        "space" => "Space".to_string(),
        "tab" => "Tab".to_string(),
        "return" | "enter" => "Return".to_string(),
        "escape" | "esc" => "Esc".to_string(),
        "minus" | "-" => "-".to_string(),
        "equal" | "=" => "=".to_string(),
        "twosuperior" | "²" => "²".to_string(),
        _ if lower.len() == 1 && lower.chars().all(|ch| ch.is_ascii_alphabetic()) => {
            lower.to_ascii_uppercase()
        }
        _ if lower.len() == 1 && lower.chars().all(|ch| ch.is_ascii_digit()) => lower,
        _ if lower.starts_with('f') => {
            let num = lower[1..]
                .parse::<u8>()
                .map_err(|_| anyhow!("unknown hotkey key '{token}' in '{raw}'"))?;
            if !(1..=24).contains(&num) {
                bail!("unknown hotkey key '{token}' in '{raw}'");
            }
            format!("F{num}")
        }
        _ => bail!("unknown hotkey key '{token}' in '{raw}'"),
    };
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::Hotkey;

    #[test]
    fn parses_basic_hotkey() {
        let hotkey = Hotkey::parse("ctrl+grave").unwrap();
        assert_eq!(hotkey.sequence(), "Ctrl+`");
    }

    #[test]
    fn parses_meta_alias() {
        let hotkey = Hotkey::parse("super+k").unwrap();
        assert_eq!(hotkey.sequence(), "Meta+K");
    }

    #[test]
    fn rejects_unknown_modifier() {
        assert!(Hotkey::parse("meh+k").is_err());
    }

    #[test]
    fn rejects_unknown_key() {
        assert!(Hotkey::parse("ctrl+backspace").is_err());
    }

    #[test]
    fn rejects_duplicate_modifier() {
        assert!(Hotkey::parse("ctrl+ctrl+k").is_err());
    }

    #[test]
    fn rejects_empty() {
        assert!(Hotkey::parse("").is_err());
    }

    #[test]
    fn rejects_separator_only_hotkey() {
        assert!(Hotkey::parse("+").is_err());
        assert!(Hotkey::parse(" + ").is_err());
    }

    #[test]
    fn parses_single_key() {
        let hotkey = Hotkey::parse("F12").unwrap();
        assert_eq!(hotkey.sequence(), "F12");
    }

    #[test]
    fn parses_control_alias_in_combo() {
        let hotkey = Hotkey::parse("control+k").unwrap();
        assert_eq!(hotkey.sequence(), "Ctrl+K");
    }

    #[test]
    fn parses_modifier_only_hotkeys() {
        for (input, sequence) in [
            ("super", "Meta"),
            ("META", "Meta"),
            ("win", "Meta"),
            ("alt", "Alt"),
            (" alt ", "Alt"),
        ] {
            let hotkey = Hotkey::parse(input).unwrap();
            assert_eq!(hotkey.sequence(), sequence, "{input}");
        }
    }

    #[test]
    fn rejects_ctrl_and_shift_alone() {
        for input in ["ctrl", "control", "shift"] {
            let err = Hotkey::parse(input).unwrap_err();
            assert!(err.to_string().contains("can't be used alone"), "{input}");
        }
    }

    #[test]
    fn rejects_modifier_only_chord() {
        let err = Hotkey::parse("ctrl+shift").unwrap_err();
        assert!(err.to_string().contains("single modifier"));
        assert!(Hotkey::parse("super+super").is_err());
    }

    #[test]
    fn rejects_side_specific_modifiers() {
        assert!(Hotkey::parse("rctrl").is_err());
        assert!(Hotkey::parse("rctrl+k").is_err());
    }

    #[test]
    fn parses_twosuperior_hotkey() {
        let hotkey1 = Hotkey::parse("ctrl+²").unwrap();
        assert_eq!(hotkey1.sequence(), "Ctrl+²");
        let hotkey2 = Hotkey::parse("ctrl+twosuperior").unwrap();
        assert_eq!(hotkey2.sequence(), "Ctrl+²");
    }
}
