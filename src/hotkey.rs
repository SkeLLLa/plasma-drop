use anyhow::{Result, anyhow, bail};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Hotkey {
    raw: String,
    sequence: String,
    modifiers: Vec<String>,
    key: String,
}

impl Hotkey {
    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn sequence(&self) -> &str {
        &self.sequence
    }

    #[allow(dead_code)]
    pub fn modifiers(&self) -> &[String] {
        &self.modifiers
    }

    #[allow(dead_code)]
    pub fn key(&self) -> &str {
        &self.key
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

        let mut seen = HashSet::new();
        let mut modifiers = Vec::new();
        for token in &tokens[..tokens.len() - 1] {
            let normalized = match token.to_ascii_lowercase().as_str() {
                "ctrl" => "Ctrl",
                "shift" => "Shift",
                "alt" => "Alt",
                "super" | "meta" | "win" => "Meta",
                unknown => bail!("unknown hotkey modifier '{unknown}' in '{raw}'"),
            };

            if !seen.insert(normalized.to_string()) {
                bail!("duplicate hotkey modifier '{token}' in '{raw}'");
            }

            modifiers.push(normalized.to_string());
        }

        let key = normalize_key(tokens[tokens.len() - 1], &raw)?;
        let sequence = modifiers
            .iter()
            .cloned()
            .chain(std::iter::once(key.clone()))
            .collect::<Vec<_>>()
            .join("+");

        Ok(Self {
            raw,
            sequence,
            modifiers,
            key,
        })
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
    fn parses_single_key() {
        let hotkey = Hotkey::parse("F12").unwrap();
        assert_eq!(hotkey.sequence(), "F12");
    }
}
