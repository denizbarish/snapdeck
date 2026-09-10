//! The global capture shortcuts.
//!
//! Two properties are worth more here than anywhere else in the application,
//! because this is the only surface a menu bar agent has before a capture
//! exists, and a broken one is indistinguishable from an app that is not
//! running.
//!
//! A set of shortcuts is registered whole or not at all. The loop used to
//! register them one at a time and return on the first failure, which left the
//! user with two working keys, one dead one, and nothing at all to tell them
//! which was which.
//!
//! A rebind that cannot be registered puts the previous bindings back. Refusing
//! and leaving nothing bound would be the same silence with an extra step: the
//! user asked for a different key, was told no, and would then find the old key
//! had stopped working too.
//!
//! The defaults are not here. They are in `settings::Settings::default`, which
//! is the only place in this crate that states one; `Shortcuts` deliberately
//! has no `Default` impl, so there is nowhere for a second copy to hide.

use std::collections::HashMap;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shortcuts {
    pub capture_region: String,
    pub capture_window: String,
    pub capture_display: String,
}

impl Shortcuts {
    /// Capture modes, in the same order as `bindings` and `parse_all`.
    pub const MODES: [&'static str; 3] = ["region", "window", "display"];

    /// The three bindings in the order `MODES` names them.
    ///
    /// The one place the field order is written down, so a mode and the binding
    /// it stands for cannot drift apart.
    pub fn bindings(&self) -> [&str; 3] {
        [
            &self.capture_region,
            &self.capture_window,
            &self.capture_display,
        ]
    }

    pub fn parse_all(&self) -> Result<Vec<Shortcut>, String> {
        self.bindings()
            .into_iter()
            .map(|raw| {
                Shortcut::from_str(raw).map_err(|e| format!("invalid shortcut '{raw}': {e}"))
            })
            .collect()
    }

    /// The parsed bindings, once they are known to be registerable as a set.
    ///
    /// The extra thing this does over `parse_all` is refuse a set that collides
    /// with itself. Two modes on one combination cannot both be registered, and
    /// the platform's own answer to the second registration is not something a
    /// user can read; naming the two modes here is.
    ///
    /// Compared as parsed values rather than as strings, because
    /// "CmdOrCtrl+Alt+Digit4" and "Cmd+Alt+4" are different strings that are
    /// the same key.
    pub fn validate(&self) -> Result<Vec<Shortcut>, String> {
        let parsed = self.parse_all()?;
        let mut seen: HashMap<Shortcut, &'static str> = HashMap::new();
        for (index, shortcut) in parsed.iter().enumerate() {
            let mode = Self::MODES[index];
            if let Some(other) = seen.insert(*shortcut, mode) {
                return Err(format!(
                    "the {mode} and {other} shortcuts are both {}, and one combination cannot do two things",
                    self.bindings()[index]
                ));
            }
        }
        Ok(parsed)
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

/// Registers `shortcuts`, and leaves nothing registered if it cannot.
///
/// The rollback is the point. `register` can fail on any iteration, most often
/// because the combination is already held by macOS or by another application,
/// and a set that is half in force is worse than one that is not in force at
/// all: some of the user's keys work, none of them say so, and the application
/// has no way to describe the state it is in. Every caller can therefore treat
/// an error as "nothing changed".
pub fn register_shortcuts(app: &AppHandle, shortcuts: &Shortcuts) -> Result<(), String> {
    let parsed = shortcuts.validate()?;
    let manager = app.global_shortcut();
    manager.unregister_all().map_err(|e| e.to_string())?;
    for (index, shortcut) in parsed.iter().enumerate() {
        if let Err(err) = manager.register(*shortcut) {
            let mut message = format!(
                "the {} shortcut ({}) could not be registered: {err}",
                Shortcuts::MODES[index],
                shortcuts.bindings()[index]
            );
            // Reported rather than swallowed. Every caller reads an error from
            // here as "nothing is bound", and says so to the user; if the
            // rollback itself failed then some of these keys are still live,
            // and telling somebody their keyboard is empty when it is not is
            // the one place the message has to be honest.
            if let Err(cleanup_err) = manager.unregister_all() {
                message.push_str(&format!(
                    ". The bindings registered before it could not be taken back down either ({cleanup_err}), so some of them may still fire"
                ));
            }
            return Err(message);
        }
    }
    Ok(())
}

/// Takes every binding down, and says so when it cannot.
///
/// The only way back to "nothing is bound" as a deliberate state, which is what
/// a rollback needs when there were no bindings to restore.
pub fn unregister_shortcuts(app: &AppHandle) -> Result<(), String> {
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| e.to_string())
}

/// Moves from whatever is registered to `next`, or puts back what was
/// registered and says why.
///
/// `registered` is what the platform has actually accepted, which is not always
/// what the settings say: a stored binding another application had taken is
/// still in the settings file, waiting for the user to change it, while
/// something else is doing the work. Answering against that rather than against
/// the previous *settings* is what makes an unchanged set still get a second
/// attempt, instead of the application reporting success over a keyboard where
/// nothing is bound.
///
/// The answer is a pair: what is bound now, which the caller has to record
/// whether this succeeded or not, and why the user did not get what they asked
/// for. The user is never left with a keyboard they cannot explain: either the
/// new bindings are in force, or the ones that were in force still are, and the
/// error names which.
pub fn rebind(
    app: &AppHandle,
    registered: Option<&Shortcuts>,
    next: &Shortcuts,
) -> (Option<Shortcuts>, Result<(), String>) {
    rebind_with(
        |shortcuts| register_shortcuts(app, shortcuts),
        registered,
        next,
    )
}

/// The decision inside `rebind`, with the registration injected.
///
/// Split out because the property worth testing, that a refused rebind leaves
/// the previous bindings in force, cannot be observed through a live
/// `GlobalShortcut`: registering a combination the platform will refuse is not
/// something a test can arrange, and reading back what is registered is not
/// something the plugin offers.
fn rebind_with<F>(
    mut register: F,
    registered: Option<&Shortcuts>,
    next: &Shortcuts,
) -> (Option<Shortcuts>, Result<(), String>)
where
    F: FnMut(&Shortcuts) -> Result<(), String>,
{
    // Nothing to do, and nothing to risk: re-registering the set that is
    // already bound would unregister working bindings first, for no gain. The
    // test is against what is *bound*, not against what the previous settings
    // said: a set that failed to register at launch is unchanged in the file
    // and still deserves another attempt on the next save.
    if registered == Some(next) {
        return (registered.cloned(), Ok(()));
    }
    let Err(err) = register(next) else {
        return (Some(next.clone()), Ok(()));
    };
    let Some(previous) = registered else {
        // Nothing was bound before this, so there is nothing to put back and no
        // rollback to report; the keyboard is exactly as empty as it was.
        return (
            None,
            Err(format!(
                "{err}. No capture shortcut is bound; use the menu bar item until this is fixed."
            )),
        );
    };
    match register(previous) {
        Ok(()) => (
            Some(previous.clone()),
            Err(format!("{err}. Your previous shortcuts are still in force.")),
        ),
        Err(restore_err) => (
            None,
            Err(format!(
                "{err}. The previous shortcuts could not be put back either ({restore_err}), so no capture shortcut is bound; use the menu bar item until this is fixed."
            )),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::settings::Settings;

    fn defaults() -> Shortcuts {
        Settings::default().shortcuts
    }

    #[test]
    fn defaults_are_parseable_shortcuts() {
        let parsed = defaults().parse_all().expect("defaults must parse");
        assert_eq!(parsed.len(), 3);
    }

    #[test]
    fn invalid_shortcut_is_reported_with_its_value() {
        let shortcuts = Shortcuts {
            capture_region: "NotAKey+++".to_string(),
            ..defaults()
        };
        let err = shortcuts.parse_all().unwrap_err();
        assert!(
            err.contains("NotAKey+++"),
            "error should name the bad value: {err}"
        );
    }

    #[test]
    fn modes_line_up_with_parsed_shortcuts() {
        let shortcuts = defaults();
        let parsed = shortcuts.parse_all().expect("defaults must parse");
        assert_eq!(parsed.len(), Shortcuts::MODES.len());
        // Every position, not just one, and against literals rather than
        // `MODES`: `mode_for_parsed` reads the mode out of `MODES`, so asserting
        // one against the other holds for any ordering. Without an independent
        // oracle, swapping MODES 0 and 2 passes here and silently makes the
        // region shortcut capture the whole screen.
        for (index, expected) in ["region", "window", "display"].iter().enumerate() {
            assert_eq!(shortcuts.mode_for_parsed(&parsed[index]), Some(*expected));
        }
    }

    #[test]
    fn mode_for_parsed_ignores_foreign_shortcuts() {
        let foreign = Shortcut::from_str("CmdOrCtrl+Alt+K").expect("valid");
        assert_eq!(defaults().mode_for_parsed(&foreign), None);
    }

    #[test]
    fn defaults_do_not_collide_with_each_other() {
        defaults()
            .validate()
            .expect("the built-in bindings must be registerable as a set");
    }

    /// Two modes on one combination is a conflict the platform answers for with
    /// an error nobody can read, so it is caught before anything is registered.
    /// The two spellings are deliberately different strings for the same key:
    /// a string comparison would miss this.
    #[test]
    fn a_set_that_collides_with_itself_is_refused_by_name() {
        let shortcuts = Shortcuts {
            capture_region: "CmdOrCtrl+Alt+Digit7".to_string(),
            capture_window: "Cmd+Alt+7".to_string(),
            capture_display: "CmdOrCtrl+Alt+Digit9".to_string(),
        };
        let err = shortcuts
            .validate()
            .expect_err("one combination cannot drive two modes");
        assert!(err.contains("window"), "{err}");
        assert!(err.contains("region"), "{err}");
    }

    /// A recorder of what a fake registration left in force, so a test can ask
    /// the question the user would: which shortcuts does the application
    /// actually have now?
    struct Registrar {
        /// The set the platform refuses, by its region binding.
        refuse: String,
        in_force: Option<Shortcuts>,
    }

    impl Registrar {
        fn register(&mut self, shortcuts: &Shortcuts) -> Result<(), String> {
            // The real one unregisters everything before it registers anything,
            // so a refusal leaves nothing bound. The fake has to model that, or
            // the restore would look like it worked when it never ran.
            self.in_force = None;
            if shortcuts.capture_region == self.refuse {
                return Err("the region shortcut is already taken".to_string());
            }
            self.in_force = Some(shortcuts.clone());
            Ok(())
        }
    }

    /// The rule this module exists for. A rebind that cannot be registered is
    /// refused *and* the previous bindings come back; leaving nothing bound
    /// would be the silent dead keys with an extra step.
    #[test]
    fn a_refused_rebind_puts_the_previous_bindings_back() {
        let previous = defaults();
        let taken = Shortcuts {
            capture_region: "CmdOrCtrl+Shift+Digit3".to_string(),
            ..previous.clone()
        };
        let mut registrar = Registrar {
            refuse: taken.capture_region.clone(),
            in_force: Some(previous.clone()),
        };

        let (bound, outcome) = rebind_with(
            |shortcuts| registrar.register(shortcuts),
            Some(&previous),
            &taken,
        );

        let err = outcome.expect_err("a combination the platform refuses must be refused here");
        assert!(
            err.contains("already taken"),
            "the reason is the user's: {err}"
        );
        assert_eq!(
            registrar.in_force,
            Some(previous.clone()),
            "the previous bindings must be in force again, not nothing"
        );
        assert_eq!(
            bound,
            Some(previous),
            "and the caller has to be told that is what is bound"
        );
    }

    #[test]
    fn an_accepted_rebind_leaves_the_new_bindings_in_force() {
        let previous = defaults();
        let next = Shortcuts {
            capture_region: "CmdOrCtrl+Alt+KeyR".to_string(),
            ..previous.clone()
        };
        let mut registrar = Registrar {
            refuse: "nothing is refused".to_string(),
            in_force: Some(previous.clone()),
        };

        let (bound, outcome) = rebind_with(
            |shortcuts| registrar.register(shortcuts),
            Some(&previous),
            &next,
        );

        outcome.expect("a free combination must be accepted");
        assert_eq!(registrar.in_force, Some(next.clone()));
        assert_eq!(bound, Some(next));
    }

    /// Saving the settings window without touching the shortcuts must not take
    /// the working bindings down and put them back up, which is a window in
    /// which the user's key does nothing.
    #[test]
    fn the_set_that_is_already_bound_is_not_re_registered() {
        let shortcuts = defaults();
        let mut calls = 0;
        let (bound, outcome) = rebind_with(
            |_| {
                calls += 1;
                Ok(())
            },
            Some(&shortcuts),
            &shortcuts.clone(),
        );
        outcome.expect("nothing changed");
        assert_eq!(calls, 0);
        assert_eq!(bound, Some(shortcuts));
    }

    /// The launch-path bug this pair of tests exists for. A set that could not
    /// be registered is still what the settings file says, so a later save that
    /// changes something else entirely arrives with the shortcuts unchanged. It
    /// must be *attempted* rather than waved through: short-circuiting on
    /// "unchanged" alone answers `Ok` over a keyboard where these keys do
    /// nothing.
    #[test]
    fn an_unchanged_set_that_is_not_bound_is_registered_again() {
        let stored = defaults();
        let fallback = Shortcuts {
            capture_region: "CmdOrCtrl+Alt+KeyR".to_string(),
            ..stored.clone()
        };
        let mut registrar = Registrar {
            refuse: "nothing is refused".to_string(),
            in_force: Some(fallback.clone()),
        };

        let (bound, outcome) = rebind_with(
            |shortcuts| registrar.register(shortcuts),
            Some(&fallback),
            &stored,
        );

        outcome.expect("the combination is free now");
        assert_eq!(
            bound,
            Some(stored.clone()),
            "the second attempt is what puts the user's own choice back into force"
        );
        assert_eq!(registrar.in_force, Some(stored));
    }

    /// The same case when the retry fails again: the fallback that was working
    /// has to still be working afterwards, and the caller has to be told which
    /// set that is.
    #[test]
    fn a_failed_retry_of_an_unchanged_set_keeps_what_was_bound() {
        let stored = defaults();
        let fallback = Shortcuts {
            capture_region: "CmdOrCtrl+Alt+KeyR".to_string(),
            ..stored.clone()
        };
        let mut registrar = Registrar {
            refuse: stored.capture_region.clone(),
            in_force: Some(fallback.clone()),
        };

        let (bound, outcome) = rebind_with(
            |shortcuts| registrar.register(shortcuts),
            Some(&fallback),
            &stored,
        );

        outcome.expect_err("the combination is still taken");
        assert_eq!(bound, Some(fallback.clone()));
        assert_eq!(registrar.in_force, Some(fallback));
    }

    /// Nothing bound and the new set refused: there is no previous set to put
    /// back, and the message may not pretend there was one.
    #[test]
    fn a_refusal_with_nothing_bound_stays_at_nothing_bound() {
        let (bound, outcome) = rebind_with(
            |_| Err("the region shortcut is already taken".to_string()),
            None,
            &defaults(),
        );
        assert_eq!(bound, None);
        let err = outcome.expect_err("the registration failed");
        assert!(
            err.contains("No capture shortcut is bound"),
            "the user has to be told the keyboard is empty: {err}"
        );
        assert!(
            !err.contains("previous"),
            "there were no previous bindings to talk about: {err}"
        );
    }

    /// The worst case, and the one the message has to be honest about: the new
    /// set fails and the old set cannot be put back either.
    #[test]
    fn a_failed_restore_says_that_nothing_is_bound() {
        let previous = defaults();
        let next = Shortcuts {
            capture_region: "CmdOrCtrl+Alt+KeyR".to_string(),
            ..previous.clone()
        };
        let (bound, outcome) = rebind_with(
            |_| Err("the manager is gone".to_string()),
            Some(&previous),
            &next,
        );
        assert_eq!(bound, None, "nothing survived, and the caller must know");
        let err = outcome.expect_err("both registrations failed");
        assert!(
            err.contains("no capture shortcut is bound"),
            "the user has to be told the keyboard is empty: {err}"
        );
        assert!(err.contains("menu bar"), "and where to go instead: {err}");
    }
}
