use std::io::{Read, Write};
use trackfast_sync::{
    Scope,
    session::{SessionClient, SessionError},
};
use uuid::Uuid;
use zeroize::Zeroizing;

fn read_token(reader: impl Read) -> Result<Zeroizing<String>, &'static str> {
    let mut input = Zeroizing::new(String::new());
    reader
        .take(12291)
        .read_to_string(&mut input)
        .map_err(|_| "Cannot read the identity token.")?;
    let token = input
        .strip_suffix("\r\n")
        .or_else(|| input.strip_suffix('\n'))
        .unwrap_or(&input);
    if token.is_empty()
        || token.len() > 12288
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err("The identity token input is invalid.");
    }
    Ok(Zeroizing::new(token.to_owned()))
}
fn run() -> Result<(), &'static str> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 && !(args.len() == 4 && args[3] == "cleanup") {
        return Err("Supply the server, tenant, and person IDs.");
    }
    let scope = Scope {
        server: args[0].clone(),
        tenant_id: Uuid::parse_str(&args[1]).map_err(|_| "Invalid tenant ID.")?,
        person_id: Uuid::parse_str(&args[2]).map_err(|_| "Invalid person ID.")?,
    };
    let mut client = SessionClient::local(scope.clone())
        .map_err(|_| "Cannot open the account credential store or lock.")?;
    if args.len() == 4 {
        client
            .sign_out()
            .map_err(|_| "Session cleanup remains incomplete.")?;
        println!("{{\"signed_out\":true}}");
        return Ok(());
    }
    let challenge = client
        .challenge()
        .map_err(|_| "Cannot obtain the API sign-in challenge.")?;
    println!(
        "{}",
        serde_json::json!({"challenge_id":challenge.challenge_id,"nonce":challenge.nonce})
    );
    std::io::stdout()
        .flush()
        .map_err(|_| "Cannot send the challenge to the browser helper.")?;
    let token = read_token(std::io::stdin().lock())?;
    if let Err(error) = client.sign_in(challenge.challenge_id, &token) {
        if matches!(error, SessionError::AlreadySignedIn) {
            return Err("An existing session must be signed out first.");
        }
        let _ = client.sign_out();
        return Err("Rust sign-in failed. Check membership and retry cleanup if necessary.");
    }
    drop(token);
    drop(client);
    let mut restored = SessionClient::local(scope.clone())
        .map_err(|_| "Cannot restore the credential. Session cleanup remains necessary.")?;
    let checked = restored.check_identity();
    let cleaned = restored.sign_out();
    cleaned.map_err(|_| "Session cleanup remains incomplete.")?;
    checked.map_err(|_| "Restored session verification failed.")?;
    drop(restored);
    let mut signed_out =
        SessionClient::local(scope).map_err(|_| "Cannot check credential removal.")?;
    if !matches!(
        signed_out.check_identity(),
        Err(SessionError::SignInRequired)
    ) {
        return Err("The client retained access after sign-out.");
    }
    println!("{{\"restored\":true,\"signed_out\":true}}");
    Ok(())
}
fn main() {
    if let Err(message) = run() {
        eprintln!("FAIL: {message}");
        std::process::exit(
            if message == "An existing session must be signed out first." {
                3
            } else {
                1
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn token_input_is_bounded_and_rejects_embedded_lines() {
        assert_eq!(
            read_token("test.token.value\n".as_bytes())
                .unwrap()
                .as_str(),
            "test.token.value"
        );
        assert_eq!(
            read_token("test.token.value\r\n".as_bytes())
                .unwrap()
                .as_str(),
            "test.token.value"
        );
        for value in [
            String::new(),
            "x".repeat(12289),
            "first\nsecond".into(),
            "token secret".into(),
        ] {
            assert!(read_token(value.as_bytes()).is_err());
        }
    }
}
