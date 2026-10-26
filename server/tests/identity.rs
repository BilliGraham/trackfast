use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use tower::ServiceExt;
use trackfast_api::{AppState, router};
use uuid::Uuid;

#[tokio::test]
async fn identity_requires_authentication_and_returns_only_the_current_scope() {
    let tenant = Uuid::new_v4();
    let person = Uuid::new_v4();
    let token = "test-identity-token-with-32-characters";
    let app = router(AppState::development(
        Default::default(),
        vec![(token.into(), tenant, person)],
    ));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/me")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"tenant_id":tenant,"person_id":person})
    );
}
