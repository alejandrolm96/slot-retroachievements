//! Talking to RetroAchievements with the curl the card already ships.
//!
//! rcheevos never does its own networking: it builds a request, hands it over
//! and waits to be given a response. That is the whole contract, and it is
//! small enough that a process is a reasonable way to serve it. The
//! alternative is a TLS stack and a dependency tree in a frontend that
//! currently has neither.

use std::time::Duration;

/// `RC_API_SERVER_RESPONSE_CLIENT_ERROR`. Something on this side went wrong
/// and asking again will go the same way.
pub const CLIENT_ERROR: i32 = -1;

/// `RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR`. No answer arrived, but one
/// might next time.
pub const RETRYABLE_CLIENT_ERROR: i32 = -2;

/// Mirrors the parts of `rc_api_request_t` a caller has to act on.
#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    pub url: String,
    /// Form-encoded parameters. On a request that logs in or acts as the
    /// player, this holds the API token.
    pub post_data: Option<String>,
    pub content_type: Option<String>,
}

/// Mirrors `rc_api_server_response_t`. `status` is an HTTP code, or one of the
/// two negative constants above.
#[derive(Clone, PartialEq, Eq)]
pub struct Response {
    pub body: String,
    pub status: i32,
}

/// Where the request is sent. A trait so the client can be driven in a test
/// without a network, the same way slot's Wi-Fi jobs are.
pub trait ServerCall: Send {
    fn call(&self, request: &Request) -> Response;
}

/// A curl invocation: what to run, and what to feed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub args: Vec<String>,
    pub stdin: Option<String>,
}

pub struct Curl {
    agent: String,
    connect_timeout: Duration,
    timeout: Duration,
}

impl Curl {
    pub fn new(agent: impl Into<String>) -> Curl {
        Curl {
            agent: agent.into(),
            // The handheld takes about seven seconds to join its network after
            // the lid opens, so a request made in that window should fail and
            // be retried rather than sit waiting.
            connect_timeout: Duration::from_secs(10),
            timeout: Duration::from_secs(30),
        }
    }

    /// Everything curl is told, worked out without running anything.
    pub fn plan(&self, request: &Request) -> Plan {
        let mut args = vec![
            // Quiet, but still report transport failures on stderr.
            "-s".to_string(),
            "-S".to_string(),
            // The status code, appended after the body. Always three digits.
            "-w".to_string(),
            "%{http_code}".to_string(),
            "--connect-timeout".to_string(),
            self.connect_timeout.as_secs().to_string(),
            "--max-time".to_string(),
            self.timeout.as_secs().to_string(),
            "-A".to_string(),
            self.agent.clone(),
        ];
        if let Some(kind) = &request.content_type {
            args.push("-H".to_string());
            args.push(format!("Content-Type: {kind}"));
        }
        if request.post_data.is_some() {
            // From stdin, never as an argument: the body carries the token and
            // arguments are readable from the process list.
            args.push("--data-binary".to_string());
            args.push("@-".to_string());
        }
        // Deliberately no -L. A redirect could move a request carrying a token
        // onto a hop we did not choose.
        args.push(request.url.clone());
        Plan {
            args,
            stdin: request.post_data.clone(),
        }
    }

    /// Split what curl wrote into a body and a status.
    ///
    /// `ran` says whether curl itself started. A curl that never ran is this
    /// side's problem and will stay broken; a curl that ran and reported `000`
    /// never reached the server, which may work later.
    pub fn answer(out: &str, ran: bool) -> Response {
        let Some(split) = out.len().checked_sub(3) else {
            return Self::failed(ran);
        };
        let (body, code) = out.split_at(split);
        let Ok(status) = code.parse::<i32>() else {
            return Self::failed(ran);
        };
        if status == 0 {
            return Response {
                body: String::new(),
                status: RETRYABLE_CLIENT_ERROR,
            };
        }
        Response {
            body: body.to_string(),
            status,
        }
    }

    fn failed(ran: bool) -> Response {
        Response {
            body: String::new(),
            status: if ran {
                RETRYABLE_CLIENT_ERROR
            } else {
                CLIENT_ERROR
            },
        }
    }
}

impl ServerCall for Curl {
    fn call(&self, request: &Request) -> Response {
        let plan = self.plan(request);
        match run(&plan) {
            Some(out) => Curl::answer(&out, true),
            // curl is missing or could not be started. Asking again will not
            // help, so this is not retryable.
            None => Curl::failed(false),
        }
    }
}

fn run(plan: &Plan) -> Option<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("curl")
        .args(&plan.args)
        .stdin(match plan.stdin {
            Some(_) => Stdio::piped(),
            None => Stdio::null(),
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    if let (Some(body), Some(mut pipe)) = (&plan.stdin, child.stdin.take()) {
        // A failed write is not fatal on its own: curl may already have given
        // up, and its exit output is what gets read either way.
        let _ = pipe.write_all(body.as_bytes());
    }
    let out = child.wait_with_output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The body carries the API token on the way out.
impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let endpoint = self
            .url
            .split_once('?')
            .map_or(&*self.url, |(base, _)| base);
        f.debug_struct("Request")
            .field("url", &endpoint)
            .field("post_data", &self.post_data.as_ref().map(|_| "<redacted>"))
            .field("content_type", &self.content_type)
            .finish()
    }
}

/// The login response carries the API token on the way back.
impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("body", &format_args!("<{} bytes>", self.body.len()))
            .finish()
    }
}
