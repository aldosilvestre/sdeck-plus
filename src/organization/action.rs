use super::invalid;
use serde::{Deserialize, Serialize};
use std::io;

/// Navigation within the active profile. Folder entry is relative to the
/// current folder; only a single child name is accepted.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum NavigationAction {
    NextPage,
    PreviousPage,
    GoToPage(u16),
    OpenFolder(String),
    ParentFolder,
}

impl NavigationAction {
    /// Validate actions received from editors or persisted bindings.
    ///
    /// # Errors
    /// Rejects page zero and invalid child-folder names.
    pub fn validate(&self) -> io::Result<()> {
        match self {
            Self::GoToPage(0) => Err(invalid("pages start at 1")),
            Self::OpenFolder(name) => validate_folder(name),
            Self::NextPage | Self::PreviousPage | Self::GoToPage(_) | Self::ParentFolder => Ok(()),
        }
    }

    /// Human-readable action summary for binding editors.
    pub fn summary(&self) -> String {
        match self {
            Self::NextPage => "Next page".into(),
            Self::PreviousPage => "Previous page".into(),
            Self::GoToPage(page) => format!("Go to page {page}"),
            Self::OpenFolder(name) => format!("Open folder {name}"),
            Self::ParentFolder => "Parent folder".into(),
        }
    }
}

pub(super) fn validate_folder(name: &str) -> io::Result<()> {
    if name.trim().is_empty()
        || name != name.trim()
        || name.len() > 64
        || name == "."
        || name == ".."
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
    {
        return Err(invalid(
            "folder names must be 1-64 printable characters without separators",
        ));
    }
    Ok(())
}
