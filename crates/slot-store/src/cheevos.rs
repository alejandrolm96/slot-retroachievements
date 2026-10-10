use std::path::Path;

/// Written by hand on the card, because there is no on-screen keyboard to
/// type it with. It holds a username and the API token RetroAchievements
/// hands out at login, never a password: the token is obtained once, off the
/// device, so the password reaches neither the card nor this code.
pub const CHEEVOS_FILE: &str = "Config/retroachievements.txt";

/// The account the card plays as.
///
/// Both fields are needed or there is no account at all. A login with one of
/// them can only be refused, and asking the server to refuse it is worse than
/// not asking.
#[derive(Clone, PartialEq, Eq)]
pub struct Cheevos {
    pub user: String,
    pub token: String,
}

impl Cheevos {
    pub fn read(root: &Path) -> Option<Cheevos> {
        let entries = crate::ini::read(root, CHEEVOS_FILE);
        let field = |key: &str| {
            entries
                .get(key)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        Some(Cheevos {
            user: field("user")?,
            token: field("token")?,
        })
    }
}

/// The token is a bearer credential, so it is named rather than shown. Slot
/// logs to a file on the card and traces to stderr, and a derived Debug would
/// put a working credential in both.
impl std::fmt::Debug for Cheevos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cheevos")
            .field("user", &self.user)
            .field("token", &"<redacted>")
            .finish()
    }
}
