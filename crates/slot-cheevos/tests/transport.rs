//! What rcheevos needs to talk to RetroAchievements, and what it must never
//! let slip while doing it.
//!
//! The request body carries the API token, and the login response carries it
//! back. Both are bearer credentials, so neither may reach a log, and the body
//! must not reach the process list either: `ps` is readable by anything on the
//! handheld.

use slot_cheevos::{Curl, Request, Response, CLIENT_ERROR, RETRYABLE_CLIENT_ERROR};

fn curl() -> Curl {
    Curl::new("Slot/0.1")
}

fn post(body: &str) -> Request {
    Request {
        url: "https://retroachievements.org/dorequest.php".into(),
        post_data: Some(body.into()),
        content_type: Some("application/x-www-form-urlencoded".into()),
    }
}

fn get() -> Request {
    Request {
        url: "https://media.retroachievements.org/Badge/1.png".into(),
        post_data: None,
        content_type: None,
    }
}

#[test]
fn the_request_body_goes_in_on_stdin_and_never_into_the_arguments() {
    // r=login2&u=...&t=<token>. An argument is visible to every process on
    // the handheld for as long as the request takes.
    let plan = curl().plan(&post("r=login2&u=Alejandro&t=s3cr3t"));
    assert!(
        !plan.args.iter().any(|a| a.contains("s3cr3t")),
        "the token is in the command line: {:?}",
        plan.args
    );
    assert_eq!(
        plan.stdin.as_deref(),
        Some("r=login2&u=Alejandro&t=s3cr3t"),
        "the body has to reach curl somehow"
    );
    assert!(
        plan.args.iter().any(|a| a == "--data-binary"),
        "nothing told curl to read the body: {:?}",
        plan.args
    );
}

#[test]
fn a_request_with_no_body_sends_no_body() {
    let plan = curl().plan(&get());
    assert_eq!(plan.stdin, None);
    assert!(
        !plan.args.iter().any(|a| a == "--data-binary"),
        "a GET was turned into a POST: {:?}",
        plan.args
    );
}

#[test]
fn the_content_type_is_passed_as_a_header() {
    let plan = curl().plan(&post("r=ping"));
    let args = plan.args.join(" ");
    assert!(
        args.contains("Content-Type: application/x-www-form-urlencoded"),
        "the server was not told what the body is: {args}"
    );
}

#[test]
fn the_client_names_itself_because_the_server_asks_callers_to() {
    let plan = curl().plan(&post("r=ping"));
    assert!(
        plan.args.iter().any(|a| a == "Slot/0.1"),
        "no user agent was sent: {:?}",
        plan.args
    );
}

#[test]
fn a_request_cannot_hang_forever() {
    // This runs on a thread of its own, but a request with no deadline would
    // keep that thread and the work queued behind it indefinitely.
    let plan = curl().plan(&post("r=ping"));
    let args = plan.args.join(" ");
    assert!(args.contains("--max-time"), "no overall deadline: {args}");
    assert!(
        args.contains("--connect-timeout"),
        "no connect deadline: {args}"
    );
}

#[test]
fn redirects_are_not_followed_because_one_could_point_at_plain_http() {
    let plan = curl().plan(&post("r=ping"));
    assert!(
        !plan.args.iter().any(|a| a == "-L" || a == "--location"),
        "a redirect could move a token to an unencrypted hop: {:?}",
        plan.args
    );
}

#[test]
fn the_status_is_taken_off_the_tail_of_what_curl_wrote() {
    // curl is asked to append %{http_code}, which is always three digits, so
    // the body is everything before them.
    let r = Curl::answer("{\"Success\":true}200", true);
    assert_eq!(r.status, 200);
    assert_eq!(r.body, "{\"Success\":true}");
}

#[test]
fn an_error_body_is_kept_because_the_server_explains_itself_in_it() {
    let r = Curl::answer("{\"Error\":\"invalid user/token\"}401", true);
    assert_eq!(r.status, 401);
    assert_eq!(
        r.body, "{\"Error\":\"invalid user/token\"}",
        "the reason the login failed was thrown away"
    );
}

#[test]
fn an_empty_body_with_a_status_is_still_an_answer() {
    let r = Curl::answer("204", true);
    assert_eq!(r.status, 204);
    assert_eq!(r.body, "");
}

#[test]
fn a_connection_that_never_happened_is_worth_retrying() {
    // curl writes 000 when it never got a response. The handheld joins its
    // network some seconds after waking, so this is the ordinary case right
    // after the lid opens, not a real failure.
    let r = Curl::answer("000", false);
    assert_eq!(
        r.status, RETRYABLE_CLIENT_ERROR,
        "a request made before the network was up would be given up on"
    );
}

#[test]
fn output_that_is_not_a_status_at_all_is_a_client_error() {
    for out in ["", "x", "ok"] {
        let r = Curl::answer(out, false);
        assert_eq!(
            r.status, CLIENT_ERROR,
            "{out:?} was read as something meaningful"
        );
    }
}

#[test]
fn neither_the_body_sent_nor_the_body_received_reaches_a_log() {
    let sent = format!("{:?}", post("r=login2&u=Alejandro&t=s3cr3t"));
    assert!(
        !sent.contains("s3cr3t"),
        "Debug printed the token being sent: {sent}"
    );
    let got = format!(
        "{:?}",
        Response {
            body: "{\"Token\":\"s3cr3t\"}".into(),
            status: 200,
        }
    );
    assert!(
        !got.contains("s3cr3t"),
        "Debug printed the token the server returned: {got}"
    );
    assert!(
        got.contains("200"),
        "the status is the useful part and should still show: {got}"
    );
}

#[test]
fn a_query_string_is_kept_out_of_a_log_in_case_it_carries_anything() {
    // rcheevos puts parameters in the body today. Showing only the endpoint
    // costs nothing and means that staying true is not load-bearing.
    let shown = format!(
        "{:?}",
        Request {
            url: "https://retroachievements.org/dorequest.php?r=login2&t=s3cr3t".into(),
            post_data: None,
            content_type: None,
        }
    );
    assert!(
        !shown.contains("s3cr3t"),
        "the query string leaked: {shown}"
    );
    assert!(
        shown.contains("dorequest.php"),
        "which endpoint was called is worth knowing: {shown}"
    );
}

/// Against the real server. Ignored, because the suite must pass with no
/// network: run with `cargo test -p slot-cheevos -- --ignored`.
///
/// It proves what no local test can: that TLS works, that the body really
/// arrives over stdin, and that the three digits come back where the parser
/// looks for them. The credentials are deliberately invalid, so this asks
/// RetroAchievements to refuse a login rather than performing one.
#[test]
#[ignore]
fn the_real_server_refuses_an_invalid_login_and_says_why() {
    use slot_cheevos::ServerCall;

    let r = curl().call(&Request {
        url: "https://retroachievements.org/dorequest.php".into(),
        post_data: Some("r=login2&u=slot-does-not-exist-xyz&t=not-a-real-token".into()),
        content_type: Some("application/x-www-form-urlencoded".into()),
    });
    assert_eq!(
        r.status, 401,
        "expected a refusal, got {r:?}; a 000 means no network"
    );
    assert!(
        r.body.contains("invalid_credentials"),
        "the server explained itself and the body did not survive: {}",
        r.body
    );
}
