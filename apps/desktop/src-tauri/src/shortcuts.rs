use std::str::FromStr;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shortcuts {
    pub capture_region: String,
    pub capture_window: String,
    pub capture_display: String,
}

impl Default for Shortcuts {
    fn default() -> Self {
        // Avoids the macOS system screenshot bindings (Cmd+Shift+3/4/5).
        Self {
            capture_region: "CmdOrCtrl+Shift+7".to_string(),
            capture_window: "CmdOrCtrl+Shift+8".to_string(),
            capture_display: "CmdOrCtrl+Shift+9".to_string(),
        }
    }
}

impl Shortcuts {
    /// Capture modes, in the same order as `parse_all` returns shortcuts.
    pub const MODES: [&'static str; 3] = ["region", "window", "display"];

    pub fn parse_all(&self) -> Result<Vec<Shortcut>, String> {
        [
            &self.capture_region,
            &self.capture_window,
            &self.capture_display,
        ]
        .into_iter()
        .map(|raw| Shortcut::from_str(raw).map_err(|e| format!("invalid shortcut '{raw}': {e}")))
        .collect()
    }

    /// Capture mode for an already parsed shortcut, or `None` when it is not
    /// ours. Compares parsed values rather than strings, so "CmdOrCtrl+Shift+7"
    /// and "Cmd+Shift+7" match the same binding.
    pub fn mode_for_parsed(&self, shortcut: &Shortcut) -> Option<&'static str> {
        let parsed = self.parse_all().ok()?;
        let index = parsed.iter().position(|candidate| candidate == shortcut)?;
        Self::MODES.get(index).copied()
    }
}

pub fn register_shortcuts(app: &AppHandle, shortcuts: &Shortcuts) -> Result<(), String> {
    let parsed = shortcuts.parse_all()?;
    let manager = app.global_shortcut();
    manager.unregister_all().map_err(|e| e.to_string())?;
    for shortcut in parsed {
        manager.register(shortcut).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_parseable_shortcuts() {
        let shortcuts = Shortcuts::default();
        let parsed = shortcuts.parse_all().expect("defaults must parse");
        assert_eq!(parsed.len(), 3);
    }

    #[test]
    fn invalid_shortcut_is_reported_with_its_value() {
        let shortcuts = Shortcuts {
            capture_region: "NotAKey+++".to_string(),
            ..Shortcuts::default()
        };
        let err = shortcuts.parse_all().unwrap_err();
        assert!(
            err.contains("NotAKey+++"),
            "error should name the bad value: {err}"
        );
    }

    #[test]
    fn modes_line_up_with_parsed_shortcuts() {
        let shortcuts = Shortcuts::default();
        let parsed = shortcuts.parse_all().expect("defaults must parse");
        assert_eq!(parsed.len(), Shortcuts::MODES.len());
        assert_eq!(shortcuts.mode_for_parsed(&parsed[1]), Some("window"));
    }

    #[test]
    fn mode_for_parsed_ignores_foreign_shortcuts() {
        let shortcuts = Shortcuts::default();
        let foreign = Shortcut::from_str("CmdOrCtrl+Alt+K").expect("valid");
        assert_eq!(shortcuts.mode_for_parsed(&foreign), None);
    }

    #[test]
    fn defaults_do_not_collide_with_each_other() {
        let s = Shortcuts::default();
        let mut all = vec![&s.capture_region, &s.capture_window, &s.capture_display];
        all.sort();
        all.dedup();
        assert_eq!(all.len(), 3);
    }
}
