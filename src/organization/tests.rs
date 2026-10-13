use super::*;
use crate::layers::{Action, Trigger};
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};

pub(super) struct Fixture {
    pub root: PathBuf,
    pub layers: LayerStore,
    pub organization: Organization,
}

impl Fixture {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "streamdeck-organization-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let layers = LayerStore::open_at(&root).unwrap();
        let organization = Organization::in_memory(&layers);
        Self {
            root,
            layers,
            organization,
        }
    }

    pub fn page(&mut self) -> String {
        self.organization.create_page(&mut self.layers).unwrap()
    }

    pub fn folder(&mut self, name: &str) -> String {
        self.organization
            .create_folder(&mut self.layers, name)
            .unwrap()
    }

    pub fn navigate(&mut self, action: NavigationAction) -> bool {
        self.organization
            .navigate(&mut self.layers, &action)
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn application_requirements_match_focus_and_leave_base_and_manual_profiles_available() {
    let mut f = Fixture::new();
    for profile in ["Studio", "Manual"] {
        f.layers.create(profile).unwrap();
    }
    f.organization.sync(&f.layers);
    f.organization
        .set_profile_application("Studio", " OBS ")
        .unwrap();
    let focus = crate::app_focus::FocusedApplication {
        window_class: "com.obsproject.Studio".into(),
        executable: Some("obs".into()),
    };
    assert_eq!(f.organization.available_profiles(None), ["Base", "Manual"]);
    assert_eq!(
        f.organization.available_profiles(Some(&focus)),
        ["Base", "Manual", "Studio"]
    );
    assert_eq!(f.organization.profile_application("Studio"), Some("OBS"));
    f.organization
        .set_profile_application("Studio", "com.obsproject.Studio")
        .unwrap();
    assert!(f.organization.profile_matches("Studio", Some(&focus)));
    let before = f.organization.profile_applications.clone();
    for (profile, application) in [
        ("Base", "obs".to_owned()),
        ("Missing", "obs".to_owned()),
        ("Studio", "bad\nidentity".to_owned()),
        ("Studio", "a".repeat(129)),
    ] {
        assert!(f
            .organization
            .set_profile_application(profile, &application)
            .is_err());
        assert_eq!(f.organization.profile_applications, before);
    }
    f.organization
        .set_profile_application("Studio", "")
        .unwrap();
    assert_eq!(
        f.organization.available_profiles(None),
        ["Base", "Manual", "Studio"]
    );
}

#[test]
fn deleting_a_page_keeps_the_profile_application_until_its_last_view_is_removed() {
    let mut f = Fixture::new();
    f.layers.create("Studio").unwrap();
    assert!(f.layers.select("Studio"));
    f.organization.sync(&f.layers);
    let page = f.page();
    f.organization
        .set_profile_application("Studio", "obs")
        .unwrap();
    f.layers.remove("Studio").unwrap();
    f.organization.sync(&f.layers);
    assert_eq!(f.organization.profile_application("Studio"), Some("obs"));
    assert_eq!(
        f.organization.profile_key("Studio").as_deref(),
        Some(page.as_str())
    );
    f.layers.remove(&page).unwrap();
    f.organization.sync(&f.layers);
    assert!(f.organization.profile_application("Studio").is_none());
}

#[test]
fn profile_tabs_group_pages_and_preserve_surviving_profile_keys() {
    // Given: pages and folders share a profile but keep distinct layer keys.
    let mut f = Fixture::new();
    let second = f.page();
    f.folder("Tools");
    f.layers.create("Studio").unwrap();
    f.organization.sync(&f.layers);
    // When / Then: tabs group profiles, not every backing layer.
    assert_eq!(f.organization.profiles(), ["Base", "Studio"]);
    assert_eq!(f.organization.profile_for_key(&second), "Base");
    assert_eq!(
        f.organization.profile_key("Studio").as_deref(),
        Some("Studio")
    );
    assert_eq!(f.organization.profile_key("Base").as_deref(), Some("Base"));
    f.layers.remove("Studio").unwrap();
    f.organization.sync(&f.layers);
    assert_eq!(f.organization.profiles(), ["Base"]);
    assert!(f.organization.profile_key("Studio").is_none());
}

#[test]
fn renaming_preserves_page_identity_and_bindings_for_valid_names() {
    for name in [
        "Audio Mix".to_owned(),
        "  Mix  ".into(),
        "a".repeat(64),
        "é".repeat(32),
    ] {
        // Given
        let mut f = Fixture::new();
        let key = f.page();
        f.layers
            .bind(&key, Trigger::KeyPress(1), Action::Emit("kept".into()))
            .unwrap();
        let keys = f.layers.names();
        // When
        f.organization.rename_page(&f.layers, &name).unwrap();
        // Then
        let info = f.organization.current(&f.layers);
        assert_eq!(info.page_name, name.trim());
        assert_eq!(info.page, 2);
        assert_eq!(f.layers.active(), key);
        assert_eq!(f.layers.names(), keys);
        assert_eq!(
            f.layers.layers()[&key].bindings[&Trigger::KeyPress(1)],
            Action::Emit("kept".into())
        );
    }
}

#[test]
fn named_pages_keep_numeric_navigation_and_new_views_use_default_names() {
    // Given: adjacent pages have different display names.
    let mut f = Fixture::new();
    f.organization.rename_page(&f.layers, "Home").unwrap();
    let second = f.page();
    assert_eq!(f.organization.current(&f.layers).page_name, "Page 2");
    f.organization.rename_page(&f.layers, "Audio Mix").unwrap();
    // When: navigate by coordinates, not names.
    assert!(f.navigate(NavigationAction::PreviousPage));
    assert_eq!(f.organization.current(&f.layers).page_name, "Home");
    assert!(f.navigate(NavigationAction::NextPage));
    // Then: selection reaches the renamed page; new views do not copy its name.
    assert_eq!(f.layers.active(), second);
    assert_eq!(f.organization.current(&f.layers).page_name, "Audio Mix");
    f.folder("Tools");
    assert_eq!(f.organization.current(&f.layers).page_name, "Page 1");
    assert!(f.navigate(NavigationAction::ParentFolder));
    assert_eq!(f.organization.current(&f.layers).page_name, "Home");
    assert!(f.navigate(NavigationAction::GoToPage(2)));
    assert_eq!(f.organization.current(&f.layers).page_name, "Audio Mix");
}

#[test]
fn flat_layers_remain_independent_roots_when_adopted() {
    // Given
    let mut f = Fixture::new();
    f.layers.create("Studio / Legacy").unwrap();
    f.layers.select("Studio / Legacy");
    // When
    f.organization.sync(&f.layers);
    // Then
    let info = f.organization.current(&f.layers);
    assert_eq!(info.profile, "Studio / Legacy");
    assert!(info.folder.is_empty());
    assert_eq!((info.page, info.page_count), (1, 1));
}

#[test]
fn page_is_selected_without_copying_bindings_when_created() {
    // Given
    let mut f = Fixture::new();
    f.layers
        .bind(
            "Base",
            Trigger::KeyPress(1),
            Action::Emit("original".into()),
        )
        .unwrap();
    // When
    let key = f.page();
    // Then
    assert_eq!(f.layers.active(), key);
    assert_eq!(f.organization.current(&f.layers).page, 2);
    assert!(f.layers.layers()[&key].bindings.is_empty());
    assert_eq!(
        f.layers.layers()["Base"].bindings[&Trigger::KeyPress(1)],
        Action::Emit("original".into())
    );
}

#[test]
fn pages_stay_scoped_when_next_is_requested() {
    // Given
    let mut f = Fixture::new();
    let root_page = f.page();
    f.folder("Tools");
    f.page();
    f.layers.create("Studio").unwrap();
    f.layers.select("Studio");
    f.page();
    f.layers.select("Base");
    // When
    let changed = f.navigate(NavigationAction::NextPage);
    // Then
    assert!(changed);
    assert_eq!(f.layers.active(), root_page);
    assert_eq!(f.organization.current(&f.layers).page_count, 2);
}

#[test]
fn page_boundaries_do_not_wrap_when_navigated() {
    for (at_end, action) in [
        (false, NavigationAction::PreviousPage),
        (true, NavigationAction::NextPage),
    ] {
        // Given
        let mut f = Fixture::new();
        if at_end {
            f.page();
        }
        let active = f.layers.active().to_owned();
        // When
        let changed = f.navigate(action);
        // Then
        assert!(!changed);
        assert_eq!(f.layers.active(), active);
    }
}

#[test]
fn parent_selects_page_one_when_leaving_nested_folder() {
    // Given
    let mut f = Fixture::new();
    let parent = f.folder("Tools");
    f.page();
    f.folder("Audio");
    f.page();
    // When
    let changed = f.navigate(NavigationAction::ParentFolder);
    // Then
    assert!(changed);
    assert_eq!(f.layers.active(), parent);
    assert_eq!(f.organization.current(&f.layers).folder, ["Tools"]);
}

#[test]
fn child_entry_is_relative_when_names_match_in_different_scopes() {
    // Given
    let mut f = Fixture::new();
    let outer = f.folder("Tools");
    let inner = f.folder("Tools");
    f.layers.select(&outer);
    // When
    let changed = f.navigate(NavigationAction::OpenFolder("Tools".into()));
    // Then
    assert!(changed);
    assert_eq!(f.layers.active(), inner);
}

#[test]
fn missing_targets_do_not_select_other_views_when_navigated() {
    for action in [
        NavigationAction::GoToPage(99),
        NavigationAction::OpenFolder("Absent".into()),
        NavigationAction::ParentFolder,
    ] {
        // Given
        let mut f = Fixture::new();
        f.layers.create("Absent").unwrap();
        // When
        let changed = f.navigate(action);
        // Then
        assert!(!changed);
        assert_eq!(f.layers.active(), "Base");
    }
}

#[test]
fn exact_page_and_previous_select_existing_pages_when_requested() {
    for action in [
        NavigationAction::GoToPage(1),
        NavigationAction::PreviousPage,
    ] {
        // Given
        let mut f = Fixture::new();
        f.page();
        // When
        let changed = f.navigate(action);
        // Then
        assert!(changed);
        assert_eq!(f.layers.active(), "Base");
    }
}

#[test]
fn deleted_page_slots_are_skipped_when_navigating() {
    // Given
    let mut f = Fixture::new();
    let deleted = f.page();
    let third = f.page();
    f.layers.remove(&deleted).unwrap();
    f.organization.sync(&f.layers);
    f.layers.select("Base");
    // When
    let changed = f.navigate(NavigationAction::NextPage);
    // Then
    assert!(changed);
    assert_eq!(f.layers.active(), third);
    assert!(!f.organization.views.contains_key(&deleted));
    assert_eq!(f.organization.current(&f.layers).page_count, 3);
}

#[test]
fn friendly_keys_are_unique_and_bounded_when_legacy_names_collide() {
    // Given
    let mut f = Fixture::new();
    let profile = "é".repeat(32);
    f.layers.create(&profile).unwrap();
    f.layers.select(&profile);
    // When
    let first = f.page();
    let second = f.page();
    // Then
    assert_ne!(first, second);
    assert!(first.len() <= 64 && second.len() <= 64);
    assert!(!first.chars().any(char::is_control));
    assert_eq!(f.organization.current(&f.layers).profile, profile);
}
