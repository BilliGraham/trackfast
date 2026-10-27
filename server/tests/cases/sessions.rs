use super::*;

#[tokio::test]
async fn development_mode_does_not_issue_google_sessions() {
    let f = Fixture::new().await;
    for path in [
        "/v1/auth/google/challenge",
        "/v1/auth/google/exchange",
        "/v1/auth/refresh",
        "/v1/auth/logout",
    ] {
        let result = f.send("POST", path, None, None, None, json!({})).await;
        assert_eq!(result.0, StatusCode::UNAUTHORIZED, "{path}");
    }
    f.close().await;
}
