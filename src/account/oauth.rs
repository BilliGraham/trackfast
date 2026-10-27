use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub(super) fn authorization_url(
    client: &str,
    redirect: &str,
    state: &str,
    nonce: &str,
    verifier: &str,
) -> String {
    let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth").unwrap();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    url.query_pairs_mut().extend_pairs([
        ("client_id", client),
        ("redirect_uri", redirect),
        ("response_type", "code"),
        ("scope", "openid email"),
        ("state", state),
        ("nonce", nonce),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("prompt", "select_account"),
    ]);
    url.into()
}
pub(super) fn callback(request: &str, host: &str, state: &str) -> Result<String, &'static str> {
    let invalid = "The sign-in callback is invalid.";
    if request.len() > 16384 || !request.ends_with("\r\n\r\n") {
        return Err(invalid);
    }
    let mut lines = request.split("\r\n");
    let parts: Vec<_> = lines.next().ok_or(invalid)?.split(' ').collect();
    if parts.len() != 3
        || parts[0] != "GET"
        || parts[2] != "HTTP/1.1"
        || !parts[1].starts_with("/google/callback?")
        || parts[1].contains(['#', '\\'])
    {
        return Err(invalid);
    }
    let mut hosts = 0;
    for line in lines.filter(|l| !l.is_empty()) {
        let (name, value) = line.split_once(':').ok_or(invalid)?;
        if name.eq_ignore_ascii_case("host") {
            hosts += 1;
            if value.trim() != host {
                return Err(invalid);
            }
        }
        if name.eq_ignore_ascii_case("transfer-encoding")
            || (name.eq_ignore_ascii_case("content-length") && value.trim() != "0")
        {
            return Err(invalid);
        }
    }
    if hosts != 1 {
        return Err(invalid);
    }
    let url = url::Url::parse(&format!("http://{host}{}", parts[1])).map_err(|_| invalid)?;
    let pairs: Vec<_> = url.query_pairs().collect();
    if pairs.len() > 20 {
        return Err(invalid);
    }
    let states: Vec<_> = pairs.iter().filter(|(k, _)| k == "state").collect();
    let codes: Vec<_> = pairs.iter().filter(|(k, _)| k == "code").collect();
    if states.len() != 1 || states[0].1 != state {
        return Err(invalid);
    }
    let errors: Vec<_> = pairs.iter().filter(|(k, _)| k == "error").collect();
    if errors.len() == 1 && codes.is_empty() {
        return Err("Google sign-in was declined.");
    }
    if !errors.is_empty() {
        return Err(invalid);
    }
    if states.len() != 1
        || states[0].1 != state
        || codes.len() != 1
        || codes[0].1.is_empty()
        || codes[0].1.len() > 4096
        || codes[0].1.chars().any(char::is_control)
    {
        return Err(invalid);
    }
    Ok(codes[0].1.to_string())
}
fn random() -> Zeroizing<String> {
    Zeroizing::new(format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    ))
}
fn wait_for_code(
    listener: &TcpListener,
    state: &str,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<Zeroizing<String>, &'static str> {
    let host = listener
        .local_addr()
        .map_err(|_| "The sign-in listener failed.")?
        .to_string();
    listener
        .set_nonblocking(true)
        .map_err(|_| "The sign-in listener failed.")?;
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline && !cancel.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream
                    .set_nonblocking(false)
                    .map_err(|_| "The sign-in listener failed.")?;
                stream
                    .set_read_timeout(Some(Duration::from_millis(200)))
                    .map_err(|_| "The sign-in listener failed.")?;
                stream
                    .set_write_timeout(Some(Duration::from_millis(200)))
                    .map_err(|_| "The sign-in listener failed.")?;
                let mut request = Zeroizing::new(Vec::new());
                let started = Instant::now();
                while request.len() < 16384
                    && started.elapsed() < Duration::from_secs(2)
                    && !cancel.load(Ordering::Acquire)
                    && Instant::now() < deadline
                {
                    let mut buffer = [0; 512];
                    match stream.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(count) => request.extend_from_slice(&buffer[..count]),
                        Err(error)
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) =>
                        {
                            continue;
                        }
                        Err(_) => break,
                    }
                    if request.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                let result = std::str::from_utf8(&request)
                    .map_err(|_| "The sign-in callback is invalid.")
                    .and_then(|r| callback(r, &host, state));
                let declined = matches!(result, Err("Google sign-in was declined."));
                let (status, body) = if result.is_ok() || declined {
                    (
                        "200 OK",
                        "Sign-in response received. Return to Trackfast to check the result.",
                    )
                } else {
                    ("400 Bad Request", "Sign-in callback rejected.")
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                if declined {
                    return Err("Google sign-in was declined.");
                }
                if let Ok(code) = result {
                    return Ok(Zeroizing::new(code));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50))
            }
            Err(_) => return Err("The sign-in listener failed."),
        }
    }
    Err(if cancel.load(Ordering::Acquire) {
        "Sign-in cancelled."
    } else {
        "Sign-in timed out. Try again."
    })
}
pub(super) fn authorize(
    client: &str,
    secret: &str,
    nonce: &str,
    expires_in: u64,
    cancel: &AtomicBool,
    open: impl FnOnce(String),
) -> Result<Zeroizing<String>, &'static str> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|_| "Trackfast cannot start the sign-in listener.")?;
    let redirect = format!(
        "http://{}/google/callback",
        listener
            .local_addr()
            .map_err(|_| "The sign-in listener failed.")?
    );
    let state = random();
    let verifier = random();
    if cancel.load(Ordering::Acquire) {
        return Err("Sign-in cancelled.");
    }
    open(authorization_url(
        client, &redirect, &state, nonce, &verifier,
    ));
    let code = wait_for_code(
        &listener,
        &state,
        cancel,
        Duration::from_secs(expires_in.min(300)),
    )?;
    if cancel.load(Ordering::Acquire) {
        return Err("Sign-in cancelled.");
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .max_redirects(0)
        .proxy(None)
        .http_status_as_error(false)
        .build()
        .into();
    let mut fields = vec![
        ("client_id", client),
        ("code", &code),
        ("code_verifier", &verifier),
        ("redirect_uri", &redirect),
        ("grant_type", "authorization_code"),
    ];
    if !secret.is_empty() {
        fields.push(("client_secret", secret));
    }
    let mut response = agent
        .post("https://oauth2.googleapis.com/token")
        .send_form(fields)
        .map_err(|_| "Google sign-in could not finish. Check the connection and try again.")?;
    if response.status() != 200 {
        return Err(
            "Google did not accept the sign-in exchange. Check the desktop client secret and try again.",
        );
    }
    #[derive(serde::Deserialize)]
    struct Token {
        id_token: String,
    }
    let token: Token = response
        .body_mut()
        .with_config()
        .limit(32768)
        .read_json()
        .map_err(|_| "Google returned an invalid sign-in response.")?;
    let token = Zeroizing::new(token.id_token);
    if token.is_empty() || token.len() > 12288 || token.chars().any(char::is_control) {
        return Err("Google returned an invalid identity token.");
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpStream, sync::Arc};

    #[test]
    fn listener_rejects_wrong_state_and_accepts_only_the_valid_callback() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            wait_for_code(
                &listener,
                "expected",
                &AtomicBool::new(false),
                Duration::from_secs(3),
            )
        });
        for (state, status) in [("wrong", "400 Bad Request"), ("expected", "200 OK")] {
            let mut stream = TcpStream::connect(address).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            write!(stream,"GET /google/callback?state={state}&code=test-code HTTP/1.1\r\nHost: {address}\r\n\r\n").unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            assert!(response.contains(status));
            assert!(!response.contains("test-code"));
        }
        assert_eq!(worker.join().unwrap().unwrap().as_str(), "test-code");
    }

    #[test]
    fn cancellation_closes_the_listener_without_a_callback() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker = std::thread::spawn(move || {
            wait_for_code(
                &listener,
                "expected",
                &worker_cancel,
                Duration::from_secs(3),
            )
        });
        cancel.store(true, Ordering::Release);
        assert_eq!(worker.join().unwrap().unwrap_err(), "Sign-in cancelled.");
        assert!(TcpStream::connect(address).is_err());
    }

    #[test]
    fn a_valid_decline_stops_waiting_for_signin() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            wait_for_code(
                &listener,
                "expected",
                &AtomicBool::new(false),
                Duration::from_secs(1),
            )
        });
        let mut stream = TcpStream::connect(address).unwrap();
        write!(stream,"GET /google/callback?state=expected&error=access_denied HTTP/1.1\r\nHost: {address}\r\n\r\n").unwrap();
        assert_eq!(
            worker.join().unwrap().unwrap_err(),
            "Google sign-in was declined."
        );
    }
}
