//! The RetroAchievements account the card plays as.
//!
//! The token is a bearer credential: anything holding it can act as the player
//! until it is revoked. It is read and passed along, never printed.

use slot_store::{Cheevos, CHEEVOS_FILE};
use tempfile::TempDir;

fn card(text: &str) -> TempDir {
    let d = TempDir::new().expect("tmp");
    let path = d.path().join(CHEEVOS_FILE);
    std::fs::create_dir_all(path.parent().expect("Config")).expect("mkdir");
    std::fs::write(&path, text).expect("write");
    d
}

#[test]
fn a_card_with_a_user_and_a_token_can_play_for_that_account() {
    let d = card("user = Alejandro\ntoken = abc123\n");
    let c = Cheevos::read(d.path()).expect("the file names both, so it should read");
    assert_eq!(c.user, "Alejandro");
    assert_eq!(c.token, "abc123");
}

#[test]
fn a_card_with_no_file_plays_for_nobody() {
    let d = TempDir::new().expect("tmp");
    assert!(
        Cheevos::read(d.path()).is_none(),
        "an absent file produced an account, which would be sent to the server"
    );
}

#[test]
fn a_user_with_no_token_cannot_log_in_so_it_is_not_an_account() {
    let d = card("user = Alejandro\n");
    assert!(
        Cheevos::read(d.path()).is_none(),
        "half an account would be a login attempt that can only fail"
    );
}

#[test]
fn a_token_with_no_user_is_not_an_account_either() {
    let d = card("token = abc123\n");
    assert!(Cheevos::read(d.path()).is_none());
}

#[test]
fn an_empty_value_is_the_same_as_having_said_nothing() {
    // A card edited by hand is the only way this file is written, so a key
    // left blank is the likeliest mistake in it.
    for text in [
        "user =\ntoken = abc123\n",
        "user = Alejandro\ntoken =\n",
        "user =   \ntoken =   \n",
    ] {
        let d = card(text);
        assert!(
            Cheevos::read(d.path()).is_none(),
            "a blank value was taken as real in {text:?}"
        );
    }
}

#[test]
fn comments_and_the_order_of_the_keys_do_not_matter() {
    let d = card("# my account\n\ntoken = abc123\n; and the name\nuser = Alejandro\n");
    let c = Cheevos::read(d.path()).expect("comments are not content");
    assert_eq!(c.user, "Alejandro");
    assert_eq!(c.token, "abc123");
}

#[test]
fn the_token_never_reaches_a_log_even_when_the_account_is_printed() {
    // Slot logs to a file on the card and traces to stderr. A Debug that
    // spelled the token out would put a working credential in both.
    let c = Cheevos {
        user: "Alejandro".into(),
        token: "s3cr3t-token".into(),
    };
    let shown = format!("{c:?}");
    assert!(
        !shown.contains("s3cr3t-token"),
        "Debug printed the token: {shown}"
    );
    assert!(
        shown.contains("Alejandro"),
        "the user is not a secret and is worth logging: {shown}"
    );
}

#[test]
fn the_file_sits_with_the_other_card_settings() {
    assert_eq!(CHEEVOS_FILE, "Config/retroachievements.txt");
}
