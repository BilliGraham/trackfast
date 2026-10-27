use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use jsonwebtoken::{EncodingKey, Header, encode};
use tower::ServiceExt;

fn claims(nonce: &str) -> Value {
    let now = jsonwebtoken::get_current_timestamp();
    json!({"iss":"https://accounts.google.com","sub":"approved-subject","aud":"test-client","iat":now,"exp":now+300,"nonce":nonce,"email_verified":true})
}
fn signed(value: Value) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("test-key".into());
    encode(
        &header,
        &value,
        &EncodingKey::from_rsa_pem(include_bytes!("auth_test_key.pem")).unwrap(),
    )
    .unwrap()
}
fn verifier() -> Google {
    let mut google = Google::new("test-client".into()).unwrap();
    google.cache = tokio::sync::Mutex::new(KeyCache {
        keys: vec![(
            "test-key".into(),
            DecodingKey::from_rsa_pem(include_bytes!("auth_test_public.pem")).unwrap(),
        )],
        expires: Instant::now() + Duration::from_secs(300),
        retry_after: Instant::now() + Duration::from_secs(300),
    });
    google
}

#[tokio::test]
async fn google_tokens_require_signature_claims_and_nonce() {
    let google = verifier();
    assert_eq!(
        google
            .verify(&signed(claims("nonce")), "nonce")
            .await
            .unwrap(),
        "approved-subject"
    );
    for (field, value) in [
        ("iss", json!("https://attacker.invalid")),
        ("aud", json!("other-client")),
        ("azp", json!("other-client")),
        ("exp", json!(1)),
        ("iat", json!(jsonwebtoken::get_current_timestamp() + 3600)),
        ("nonce", json!("other")),
        ("email_verified", json!(false)),
        ("sub", json!("")),
    ] {
        let mut value_claims = claims("nonce");
        value_claims[field] = value;
        assert!(
            google.verify(&signed(value_claims), "nonce").await.is_err(),
            "{field}"
        );
    }
    for field in ["iss", "aud", "iat", "exp", "nonce", "sub", "email_verified"] {
        let mut value = claims("nonce");
        value.as_object_mut().unwrap().remove(field);
        assert!(
            google.verify(&signed(value), "nonce").await.is_err(),
            "missing {field}"
        );
    }
    let token = signed(claims("nonce"));
    let mut parts: Vec<_> = token.split('.').map(str::to_owned).collect();
    let replacement = if parts[2].starts_with('A') { "B" } else { "A" };
    parts[2].replace_range(..1, replacement);
    assert!(google.verify(&parts.join("."), "nonce").await.is_err());
}

async fn send(
    state: &AppState,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .uri(path)
        .method(
            if path.starts_with("/v1/me") || path == "/v1/admin/projects" {
                "GET"
            } else {
                "POST"
            },
        )
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = crate::router(state.clone())
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let bytes = to_bytes(response.into_body(), 16384).await.unwrap();
    (
        status,
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        },
    )
}

#[tokio::test]
async fn session_lifecycle_enforces_membership_rotation_and_revocation() {
    let mut config: tokio_postgres::Config = std::env::var("TRACKFAST_TEST_DATABASE_URL")
        .unwrap()
        .parse()
        .unwrap();
    let (admin, connection) = config.connect(tokio_postgres::NoTls).await.unwrap();
    tokio::spawn(async move {
        connection.await.unwrap();
    });
    let schema = format!("test_{}", Uuid::new_v4().simple());
    admin
        .batch_execute(&format!("CREATE SCHEMA {schema}"))
        .await
        .unwrap();
    config.options(format!("-c search_path={schema}"));
    let mut state = AppState::google(config.clone(), "test-client".into()).unwrap();
    state.google = Some(Arc::new(verifier()));
    state.migrate().await.unwrap();
    let client = state.connect().await.unwrap();
    let tenant = Uuid::new_v4();
    let person = Uuid::new_v4();
    let login = |challenge: &Value| json!({"challenge_id":challenge["challenge_id"],"id_token":signed(claims(challenge["nonce"].as_str().unwrap()))});
    let challenge = send(&state, "/v1/auth/google/challenge", None, json!({}))
        .await
        .1;
    assert_eq!(
        send(&state, "/v1/auth/google/exchange", None, login(&challenge))
            .await
            .0,
        401
    );
    state
        .provision_google_member("approved-subject", tenant, person, false)
        .await
        .unwrap();
    let challenge = send(&state, "/v1/auth/google/challenge", None, json!({}))
        .await
        .1;
    let body = login(&challenge);
    let (status, tokens) = send(&state, "/v1/auth/google/exchange", None, body.clone()).await;
    assert_eq!(status, 200);
    assert_eq!(
        send(&state, "/v1/auth/google/exchange", None, body).await.0,
        401
    );
    let access = tokens["access_token"].as_str().unwrap();
    let refresh = tokens["refresh_token"].as_str().unwrap();
    let restarted = AppState::google(config, "test-client".into()).unwrap();
    restarted.migrate().await.unwrap();
    assert_eq!(
        send(&restarted, "/v1/me", Some(access), Value::Null)
            .await
            .0,
        200
    );
    assert_eq!(
        send(&state, "/v1/me", Some(access), Value::Null).await.1,
        json!({"tenant_id":tenant,"person_id":person})
    );
    assert_eq!(
        send(&state, "/v1/admin/projects", Some(access), Value::Null)
            .await
            .0,
        403
    );
    assert_eq!(
        send(&state, "/v1/me", Some(refresh), Value::Null).await.0,
        401
    );
    client
        .execute(
            "UPDATE api_auth_sessions SET access_expires_at=now()-interval '1 second'",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(
        send(&state, "/v1/me", Some(access), Value::Null).await.0,
        401
    );
    client
        .execute(
            "UPDATE api_auth_sessions SET expires_at=now()+interval '60 seconds'",
            &[],
        )
        .await
        .unwrap();
    let (status, renewed) = send(
        &state,
        "/v1/auth/refresh",
        None,
        json!({"refresh_token":refresh}),
    )
    .await;
    assert_eq!(status, 200);
    assert!(renewed["expires_in"].as_i64().unwrap() <= 60);
    assert_eq!(
        send(
            &state,
            "/v1/me",
            Some(renewed["access_token"].as_str().unwrap()),
            Value::Null
        )
        .await
        .0,
        200
    );
    assert_eq!(
        send(
            &state,
            "/v1/auth/refresh",
            None,
            json!({"refresh_token":refresh})
        )
        .await
        .0,
        401
    );
    assert_eq!(
        send(
            &state,
            "/v1/me",
            Some(renewed["access_token"].as_str().unwrap()),
            Value::Null
        )
        .await
        .0,
        401
    );
    let challenge = send(&state, "/v1/auth/google/challenge", None, json!({}))
        .await
        .1;
    let tokens = send(&state, "/v1/auth/google/exchange", None, login(&challenge))
        .await
        .1;
    let access = tokens["access_token"].as_str().unwrap();
    let refresh = tokens["refresh_token"].as_str().unwrap();
    assert_eq!(
        send(
            &state,
            "/v1/auth/logout",
            None,
            json!({"refresh_token":refresh})
        )
        .await
        .0,
        204
    );
    assert_eq!(
        send(&state, "/v1/me", Some(access), Value::Null).await.0,
        401
    );
    let challenge = send(&state, "/v1/auth/google/challenge", None, json!({}))
        .await
        .1;
    let tokens = send(&state, "/v1/auth/google/exchange", None, login(&challenge))
        .await
        .1;
    let access = tokens["access_token"].as_str().unwrap();
    let own_entry = Uuid::new_v4();
    client.execute("INSERT INTO api_entries(tenant_id,person_id,entry_id,version,data) VALUES ($1,$2,$3,1,$4)",&[&tenant,&person,&own_entry,&json!({"entry_id":own_entry})]).await.unwrap();
    assert_eq!(
        send(
            &state,
            &format!("/v1/me/entries/{own_entry}"),
            Some(access),
            Value::Null
        )
        .await
        .0,
        200
    );
    for other_tenant in [tenant, Uuid::new_v4()] {
        let other_person = Uuid::new_v4();
        let subject = other_person.to_string();
        state
            .provision_google_member(&subject, other_tenant, other_person, false)
            .await
            .unwrap();
        let challenge = send(&state, "/v1/auth/google/challenge", None, json!({}))
            .await
            .1;
        let mut other_claims = claims(challenge["nonce"].as_str().unwrap());
        other_claims["sub"] = json!(subject);
        let other_tokens = send(
            &state,
            "/v1/auth/google/exchange",
            None,
            json!({"challenge_id":challenge["challenge_id"],"id_token":signed(other_claims)}),
        )
        .await
        .1;
        let other_access = other_tokens["access_token"].as_str().unwrap();
        assert_eq!(
            send(&state, "/v1/me", Some(other_access), Value::Null)
                .await
                .1,
            json!({"tenant_id":other_tenant,"person_id":other_person})
        );
        assert_eq!(
            send(
                &state,
                &format!("/v1/me/entries/{own_entry}"),
                Some(other_access),
                Value::Null
            )
            .await
            .0,
            404
        );
        let refresh_body = json!({"refresh_token":other_tokens["refresh_token"]});
        let (first, second) = tokio::join!(
            send(&state, "/v1/auth/refresh", None, refresh_body.clone()),
            send(&state, "/v1/auth/refresh", None, refresh_body)
        );
        assert!((first.0 == 200 && second.0 == 401) || (first.0 == 401 && second.0 == 200));
        let winner = if first.0 == 200 { first.1 } else { second.1 };
        assert_eq!(
            send(
                &state,
                "/v1/me",
                Some(winner["access_token"].as_str().unwrap()),
                Value::Null
            )
            .await
            .0,
            401
        );
    }
    state
        .disable_google_member("approved-subject")
        .await
        .unwrap();
    state
        .provision_google_member("approved-subject", tenant, person, true)
        .await
        .unwrap();
    assert!(
        !client
            .query_one(
                "SELECT administrator FROM api_auth_members WHERE subject='approved-subject'",
                &[]
            )
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
    assert_eq!(
        send(
            &state,
            "/v1/me",
            Some(tokens["access_token"].as_str().unwrap()),
            Value::Null
        )
        .await
        .0,
        401
    );
    assert!(
        client
            .query(
                "SELECT access_hash FROM api_auth_sessions WHERE access_hash=$1",
                &[&access.as_bytes()]
            )
            .await
            .unwrap()
            .is_empty()
    );
    let challenge = send(&state, "/v1/auth/google/challenge", None, json!({}))
        .await
        .1;
    client
        .execute(
            "UPDATE api_auth_challenges SET expires_at=now()-interval '1 second'",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(
        send(&state, "/v1/auth/google/exchange", None, login(&challenge))
            .await
            .0,
        401
    );
    assert_eq!(
        send(
            &state,
            "/v1/auth/refresh",
            None,
            json!({"refresh_token":tokens["refresh_token"]})
        )
        .await
        .0,
        401
    );
    admin
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
        .await
        .unwrap();
}
