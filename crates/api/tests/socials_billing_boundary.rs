use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use cortex_api::state::AppState;

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).expect("JSON response")
}

async fn request(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: serde_json::Value,
) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn socials_creation_and_usage_fail_closed_without_cortex_ledger_changes() {
    std::env::set_var("CORTEX_AUTH_DISABLED", "1");

    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(temp.path().join(".cortex")).unwrap();
    let state = AppState::new(
        temp.path().join(".cortex/ledger.jsonl"),
        temp.path().to_path_buf(),
        None,
    )
    .await;
    let database = state.db.as_ref().expect("database");
    let profile = database.social_create_profile("local", "boundary", "Boundary", "");
    let profile_id = profile["id"].as_str().expect("profile id").to_string();
    database
        .init_credit_balance("local", 23)
        .expect("ledger sentinel");
    let usage_before = database.get_user_usage_summary("local", 0);
    database.upsert_subscription(&cortex_api::db::SubscriptionRecord {
        clerk_user_id: "local".to_string(),
        stripe_customer_id: "cus_existing".to_string(),
        stripe_subscription_id: Some("sub_existing".to_string()),
        plan_type: "monthly".to_string(),
        status: "active".to_string(),
        trial_end: None,
        current_period_start: None,
        current_period_end: None,
    });
    database.record_billing_event("local", "evt_existing", 699, "Existing invoice", "paid");

    let app = cortex_api::build_heyvera_router(state.clone());

    let draft_response = request(
        app.clone(),
        "POST",
        "/v1/pulse/drafts",
        serde_json::json!({ "body": "must not be created" }),
    )
    .await;
    assert_eq!(draft_response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let draft_body = body_json(draft_response).await;
    assert_eq!(draft_body["code"], "SOCIALS_BILLING_UNAVAILABLE");
    assert!(database.pulse_list_drafts(&profile_id, None).is_empty());

    let checkout_response = request(
        app.clone(),
        "POST",
        "/api/billing/checkout",
        serde_json::json!({ "plan": "monthly" }),
    )
    .await;
    assert_eq!(checkout_response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        body_json(checkout_response).await["code"],
        "SOCIALS_BILLING_UNAVAILABLE"
    );

    let usage_response = request(
        app.clone(),
        "GET",
        "/api/billing/usage",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(usage_response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        body_json(usage_response).await["code"],
        "SOCIALS_BILLING_UNAVAILABLE"
    );

    let status_response = request(
        app.clone(),
        "GET",
        "/api/billing/status",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(status_response.status(), StatusCode::OK);
    let status_body = body_json(status_response).await;
    assert_eq!(status_body["active"], true);

    let history_response =
        request(app, "GET", "/api/billing/history", serde_json::Value::Null).await;
    assert_eq!(history_response.status(), StatusCode::OK);
    let history_body = body_json(history_response).await;
    assert_eq!(history_body["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(history_body["items"][0]["description"], "Existing invoice");

    let balance = database.get_credit_balance_row("local").expect("sentinel");
    assert_eq!(balance.subscription_remaining, 23);
    assert_eq!(
        database.get_user_usage_summary("local", 0).step_count,
        usage_before.step_count
    );
    assert_eq!(
        database
            .get_subscription("local")
            .expect("subscription")
            .status,
        "active"
    );
}
