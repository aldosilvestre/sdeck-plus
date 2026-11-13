//! Which profile the focused window selects, and which one to show again when it loses focus.
//!
//! Pure decision logic over [`Organization`]; the window code only applies the answer.

use crate::{app_focus::FocusedApplication, layers::DEFAULT_LAYER, organization::Organization};

/// What the session remembers between focus changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FocusMemory {
    /// The current profile was picked by a focus match, not by the user.
    pub automatic: bool,
    /// The profile an application profile with "return to previous" replaced.
    pub previous: Option<String>,
}

impl FocusMemory {
    /// The user took control of the tabs: nothing is waiting to be restored.
    pub fn user_chose_a_profile(&mut self) {
        self.automatic = false;
        self.previous = None;
    }
}

/// The profile to show for `focus`. `manual` is the user's own tab choice, which lasts until a
/// profile matches again; it is cleared here when one does.
///
/// A matching profile replaces `current`. When that profile has "return to previous" the replaced
/// profile (unless it was itself picked by a match) is remembered and shown again once focus
/// matches nothing; otherwise, and when the remembered profile is gone or needs another
/// application, focus that matches nothing shows Base.
pub fn profile_for_focus(
    organization: &Organization,
    current: &str,
    focus: Option<&FocusedApplication>,
    manual: &mut bool,
    memory: &mut FocusMemory,
) -> String {
    if organization.profile_matches(current, focus) {
        *manual = false;
        return current.to_owned();
    }
    if let Some(profile) = organization
        .profiles()
        .into_iter()
        .find(|profile| organization.profile_matches(profile, focus))
    {
        *manual = false;
        if organization.profile_returns_to_previous(&profile)
            && !memory.automatic
            && memory.previous.is_none()
        {
            memory.previous = Some(current.to_owned());
        }
        memory.automatic = true;
        return profile;
    }
    let previous = memory.previous.take();
    if *manual {
        return current.to_owned();
    }
    let returning = memory.automatic && organization.profile_returns_to_previous(current);
    memory.automatic = false;
    let restored = previous.filter(|profile| {
        returning
            && organization
                .available_profiles(focus)
                .iter()
                .any(|available| available == profile)
    });
    match restored {
        Some(profile) => {
            // The state before the application is the user's again and stays until a profile matches.
            *manual = true;
            profile
        }
        None => DEFAULT_LAYER.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layers::LayerStore;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct Scene {
        root: std::path::PathBuf,
        organization: Organization,
        memory: FocusMemory,
        manual: bool,
        current: String,
    }

    fn app(class: &str) -> Option<FocusedApplication> {
        Some(FocusedApplication {
            window_class: class.into(),
            executable: None,
        })
    }

    impl Scene {
        /// Profiles: Base, Music (no rule), IDE (rule "idea"), Terminal (rule "kitty").
        fn new(ide_returns: bool, terminal_returns: bool) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "sdeck-focus-profile-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let mut layers = LayerStore::open_at(&root).unwrap();
            for name in ["Music", "IDE", "Terminal"] {
                layers.create(name).unwrap();
            }
            let mut organization = Organization::in_memory(&layers);
            organization.set_profile_application("IDE", "idea").unwrap();
            organization
                .set_profile_application("Terminal", "kitty")
                .unwrap();
            organization
                .set_profile_returns_to_previous("IDE", ide_returns)
                .ok();
            organization
                .set_profile_returns_to_previous("Terminal", terminal_returns)
                .ok();
            Self {
                root,
                organization,
                memory: FocusMemory::default(),
                manual: false,
                current: "Base".into(),
            }
        }

        fn focus(&mut self, class: Option<&str>) -> String {
            let focus = class.and_then(app);
            let next = profile_for_focus(
                &self.organization,
                &self.current,
                focus.as_ref(),
                &mut self.manual,
                &mut self.memory,
            );
            self.current.clone_from(&next);
            next
        }

        /// The user clicks a tab.
        fn choose(&mut self, profile: &str) {
            self.current = profile.into();
            self.manual = true;
            self.memory.user_chose_a_profile();
        }
    }

    impl Drop for Scene {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn without_the_option_leaving_the_application_goes_to_base() {
        let mut scene = Scene::new(false, false);
        scene.choose("Music");
        assert_eq!(scene.focus(Some("idea")), "IDE");
        assert_eq!(scene.focus(Some("firefox")), "Base");
        assert_eq!(scene.focus(Some("idea")), "IDE");
        assert_eq!(scene.focus(None), "Base");
    }

    /// The decision as it was before the option existed.
    fn legacy(
        organization: &Organization,
        current: &str,
        focus: Option<&FocusedApplication>,
        manual: &mut bool,
    ) -> String {
        if organization.profile_matches(current, focus) {
            *manual = false;
            current.to_owned()
        } else {
            match organization
                .profiles()
                .into_iter()
                .find(|profile| organization.profile_matches(profile, focus))
            {
                Some(profile) => {
                    *manual = false;
                    profile
                }
                None if *manual => current.to_owned(),
                None => DEFAULT_LAYER.to_owned(),
            }
        }
    }

    #[test]
    fn without_the_option_every_sequence_decides_exactly_as_before() {
        // Events: four focus changes and three tab clicks; every sequence up to length five.
        const EVENTS: usize = 7;
        for length in 1..=5u32 {
            for code in 0..(EVENTS as u32).pow(length) {
                let mut scene = Scene::new(false, false);
                let (mut old_current, mut old_manual) = ("Base".to_owned(), false);
                let mut rest = code;
                for _ in 0..length {
                    let event = (rest % EVENTS as u32) as usize;
                    rest /= EVENTS as u32;
                    match event {
                        0..=3 => {
                            let class = [Some("idea"), Some("kitty"), Some("firefox"), None][event];
                            let focus = class.and_then(app);
                            let expected = legacy(
                                &scene.organization,
                                &old_current,
                                focus.as_ref(),
                                &mut old_manual,
                            );
                            old_current.clone_from(&expected);
                            assert_eq!(scene.focus(class), expected, "sequence {code}/{length}");
                            assert_eq!(scene.manual, old_manual, "sequence {code}/{length}");
                        }
                        _ => {
                            let tab = ["Base", "Music", "IDE"][event - 4];
                            scene.choose(tab);
                            old_current = tab.to_owned();
                            old_manual = true;
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn with_the_option_leaving_the_application_returns_to_the_previous_profile() {
        let mut scene = Scene::new(true, false);
        scene.choose("Music");
        assert_eq!(scene.focus(Some("idea")), "IDE");
        assert_eq!(scene.focus(Some("idea")), "IDE", "a repeat changes nothing");
        assert_eq!(scene.focus(Some("firefox")), "Music");
        assert_eq!(scene.focus(Some("firefox")), "Music", "and it stays there");
    }

    #[test]
    fn returning_from_base_is_base_and_works_again_on_the_next_visit() {
        let mut scene = Scene::new(true, false);
        assert_eq!(scene.focus(Some("idea")), "IDE");
        assert_eq!(scene.focus(Some("firefox")), "Base");
        scene.choose("Music");
        assert_eq!(scene.focus(Some("idea")), "IDE");
        assert_eq!(
            scene.focus(None),
            "Music",
            "unavailable focus counts as leaving"
        );
    }

    #[test]
    fn going_between_application_profiles_keeps_the_original_previous_profile() {
        let mut scene = Scene::new(true, true);
        scene.choose("Music");
        assert_eq!(scene.focus(Some("idea")), "IDE");
        assert_eq!(scene.focus(Some("kitty")), "Terminal");
        assert_eq!(scene.focus(Some("firefox")), "Music");
        // An application profile without the option does not give the screen back.
        let mut scene = Scene::new(true, false);
        scene.choose("Music");
        assert_eq!(scene.focus(Some("idea")), "IDE");
        assert_eq!(scene.focus(Some("kitty")), "Terminal");
        assert_eq!(scene.focus(Some("firefox")), "Base");
    }

    #[test]
    fn a_profile_picked_by_a_match_is_never_remembered_as_previous() {
        let mut scene = Scene::new(true, true);
        assert_eq!(scene.focus(Some("kitty")), "Terminal");
        assert_eq!(scene.focus(Some("idea")), "IDE");
        assert_eq!(scene.focus(Some("firefox")), "Base", "not Terminal");
    }

    #[test]
    fn a_manual_choice_while_the_application_is_focused_wins() {
        let mut scene = Scene::new(true, false);
        scene.choose("Music");
        assert_eq!(scene.focus(Some("idea")), "IDE");
        scene.choose("Base");
        assert_eq!(scene.focus(Some("firefox")), "Base", "the user's tab stays");
        scene.choose("Music");
        assert_eq!(scene.focus(Some("firefox")), "Music");
        assert_eq!(scene.focus(Some("idea")), "IDE");
        assert_eq!(
            scene.focus(Some("firefox")),
            "Music",
            "a fresh visit remembers again"
        );
    }

    #[test]
    fn a_previous_profile_that_is_gone_or_needs_another_application_falls_back_to_base() {
        let mut scene = Scene::new(true, false);
        scene.choose("Music");
        assert_eq!(scene.focus(Some("idea")), "IDE");
        // The remembered profile is deleted while the IDE has focus.
        let mut layers = LayerStore::open_at(&scene.root).unwrap();
        layers.create("Music").ok();
        layers.remove("Music").unwrap();
        scene.organization.sync(&layers);
        assert_eq!(scene.focus(Some("firefox")), "Base");
        // The remembered profile belongs to another application.
        let mut scene = Scene::new(true, false);
        scene.choose("Terminal");
        assert_eq!(scene.focus(Some("idea")), "IDE");
        assert_eq!(scene.focus(Some("firefox")), "Base");
    }

    #[test]
    fn a_matching_current_profile_is_kept_and_clears_the_manual_choice() {
        let mut scene = Scene::new(true, false);
        scene.choose("IDE");
        assert_eq!(scene.focus(Some("idea")), "IDE");
        assert!(!scene.manual);
    }

    #[test]
    fn nothing_changes_for_the_default_behaviour_when_no_profile_uses_an_application() {
        let mut scene = Scene::new(false, false);
        scene
            .organization
            .set_profile_application("IDE", "")
            .unwrap();
        scene
            .organization
            .set_profile_application("Terminal", "")
            .unwrap();
        scene.choose("Music");
        assert_eq!(scene.focus(Some("firefox")), "Music", "manual tabs stay");
    }
}
