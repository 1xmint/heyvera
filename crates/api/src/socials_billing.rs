//! HeyVera Socials subscription handlers.
//!
//! Socials keeps its existing subscription status, history, portal, and Stripe
//! lifecycle maintenance while its future commercial model is undecided. New
//! checkout and automation usage are deliberately unavailable. This module
//! must not read or mutate the Cortex credit ledger or usage tables.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::{json, Value};

use crate::billing::{
    BillingHistoryPage, BillingHistoryQuery, CheckoutRequest, PortalResponse, SubscriptionStatus,
};
use crate::clerk::ClerkUser;
use crate::db::Database;
use crate::routes::ErrorResponse;
use crate::state::AppState;
use crate::stripe_client::StripeClient;

pub const SOCIALS_BILLING_UNAVAILABLE_CODE: &str = "SOCIALS_BILLING_UNAVAILABLE";
pub const SOCIALS_BILLING_UNAVAILABLE_MESSAGE: &str =
    "Socials billing is temporarily unavailable while its commercial model is being finalized";

type UnavailableResponse = (StatusCode, Json<Value>);

pub(crate) fn unavailable_response() -> UnavailableResponse {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": SOCIALS_BILLING_UNAVAILABLE_MESSAGE,
            "code": SOCIALS_BILLING_UNAVAILABLE_CODE,
        })),
    )
}

/// Existing subscription state remains readable from Socials-owned rows.
pub async fn get_billing_status(
    State(state): State<Arc<AppState>>,
    user: ClerkUser,
) -> Json<SubscriptionStatus> {
    crate::billing::get_billing_status(State(state), user).await
}

/// New Socials subscriptions are paused until the commercial policy is chosen.
pub async fn create_checkout(
    _user: ClerkUser,
    Json(_request): Json<CheckoutRequest>,
) -> UnavailableResponse {
    unavailable_response()
}

/// Existing subscribers retain access to Stripe's management portal.
pub async fn create_portal(
    State(state): State<Arc<AppState>>,
    user: ClerkUser,
) -> Result<Json<PortalResponse>, (StatusCode, Json<ErrorResponse>)> {
    crate::billing::create_portal(State(state), user).await
}

/// Existing Socials billing history remains readable.
pub async fn get_billing_history(
    State(state): State<Arc<AppState>>,
    user: ClerkUser,
    Query(query): Query<BillingHistoryQuery>,
) -> Json<BillingHistoryPage> {
    crate::billing::get_billing_history(State(state), user, Query(query)).await
}

/// Socials has no usage/credit product while its commercial policy is open.
pub async fn get_billing_usage(_user: ClerkUser) -> UnavailableResponse {
    unavailable_response()
}

/// Verify and apply a Stripe event without touching Cortex credits or usage.
pub async fn stripe_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let webhook_secret = state.stripe_webhook_secret.as_deref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorResponse {
            error: "webhook not configured".into(),
        }),
    ))?;

    let signature = headers
        .get("stripe-signature")
        .and_then(|value| value.to_str().ok())
        .ok_or((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "missing Stripe-Signature header".into(),
            }),
        ))?;

    StripeClient::verify_webhook_signature(&body, signature, webhook_secret).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("signature verification failed: {error}"),
            }),
        )
    })?;

    let event: Value = serde_json::from_slice(&body).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("invalid JSON: {error}"),
            }),
        )
    })?;

    let database = state.db.as_ref().ok_or((
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "database unavailable".into(),
        }),
    ))?;
    apply_stripe_event(database, &event);
    Ok(StatusCode::OK)
}

pub(crate) fn apply_stripe_event(database: &Database, event: &Value) {
    let event_type = event["type"].as_str().unwrap_or("");
    let event_id = event["id"].as_str().unwrap_or("");
    let object = &event["data"]["object"];

    match event_type {
        "checkout.session.completed" => {
            let clerk_user_id = object["metadata"]["clerk_user_id"]
                .as_str()
                .or_else(|| object["client_reference_id"].as_str());
            let customer_id = object["customer"].as_str();
            let subscription_id = object["subscription"].as_str();
            let mode = object["mode"].as_str().unwrap_or("");

            if let (Some(user_id), Some(customer_id)) = (clerk_user_id, customer_id) {
                if mode.eq_ignore_ascii_case("subscription") {
                    let subscription = crate::db::SubscriptionRecord {
                        clerk_user_id: user_id.to_string(),
                        stripe_customer_id: customer_id.to_string(),
                        stripe_subscription_id: subscription_id.map(String::from),
                        plan_type: "monthly".to_string(),
                        status: "active".to_string(),
                        trial_end: None,
                        current_period_start: None,
                        current_period_end: None,
                    };
                    database.upsert_subscription(&subscription);
                    database.record_billing_event(
                        user_id,
                        event_id,
                        0,
                        "Subscription created",
                        "completed",
                    );
                    tracing::info!(user_id, "Socials subscription created");
                }
            }
        }
        "customer.subscription.updated" => {
            let customer_id = object["customer"].as_str().unwrap_or("");
            if let Some(mut subscription) = database.get_subscription_by_customer(customer_id) {
                subscription.status = object["status"].as_str().unwrap_or("active").to_string();
                if let Some(plan) = object["items"]["data"][0]["price"]["lookup_key"].as_str() {
                    subscription.plan_type = if plan.contains("annual") {
                        "annual".to_string()
                    } else {
                        "monthly".to_string()
                    };
                }
                if let Some(end) = object["current_period_end"].as_i64() {
                    subscription.current_period_end = timestamp_to_rfc3339(end);
                }
                if let Some(start) = object["current_period_start"].as_i64() {
                    subscription.current_period_start = timestamp_to_rfc3339(start);
                }
                if let Some(trial_end) = object["trial_end"].as_i64() {
                    subscription.trial_end = timestamp_to_rfc3339(trial_end);
                }
                database.upsert_subscription(&subscription);
                tracing::info!(customer_id, "Socials subscription updated");
            }
        }
        "customer.subscription.deleted" => {
            let customer_id = object["customer"].as_str().unwrap_or("");
            if let Some(mut subscription) = database.get_subscription_by_customer(customer_id) {
                subscription.status = "cancelled".to_string();
                database.upsert_subscription(&subscription);
                database.record_billing_event(
                    &subscription.clerk_user_id,
                    event_id,
                    0,
                    "Subscription cancelled",
                    "completed",
                );
                tracing::info!(customer_id, "Socials subscription cancelled");
            }
        }
        "invoice.paid" => {
            let customer_id = object["customer"].as_str().unwrap_or("");
            let amount = object["amount_paid"].as_i64().unwrap_or(0);
            if let Some(subscription) = database.get_subscription_by_customer(customer_id) {
                database.record_billing_event(
                    &subscription.clerk_user_id,
                    event_id,
                    amount,
                    "Invoice paid",
                    "paid",
                );
            }
        }
        "invoice.payment_failed" => {
            let customer_id = object["customer"].as_str().unwrap_or("");
            if let Some(mut subscription) = database.get_subscription_by_customer(customer_id) {
                subscription.status = "past_due".to_string();
                database.upsert_subscription(&subscription);
                database.record_billing_event(
                    &subscription.clerk_user_id,
                    event_id,
                    0,
                    "Payment failed",
                    "failed",
                );
                tracing::warn!(customer_id, "Socials subscription payment failed");
            }
        }
        _ => tracing::debug!(event_type, "unhandled Socials Stripe event"),
    }
}

fn timestamp_to_rfc3339(timestamp: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(timestamp, 0).map(|date| date.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Database {
        let directory = tempfile::tempdir().unwrap().keep();
        Database::open(&directory.join("socials-billing.sqlite"))
    }

    #[test]
    fn unavailable_contract_is_stable() {
        let (status, Json(body)) = unavailable_response();
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["code"], SOCIALS_BILLING_UNAVAILABLE_CODE);
        assert_eq!(body["error"], SOCIALS_BILLING_UNAVAILABLE_MESSAGE);
    }

    #[test]
    fn checkout_event_preserves_subscription_and_history() {
        let database = test_db();
        let user_id = "socials_checkout_user";

        apply_stripe_event(
            &database,
            &json!({
                "id": "evt_socials_checkout",
                "type": "checkout.session.completed",
                "data": { "object": {
                    "mode": "subscription",
                    "customer": "cus_socials",
                    "subscription": "sub_socials",
                    "metadata": { "clerk_user_id": user_id }
                }}
            }),
        );

        let subscription = database.get_subscription(user_id).expect("subscription");
        assert_eq!(subscription.status, "active");
        assert_eq!(subscription.stripe_customer_id, "cus_socials");
        assert_eq!(database.get_billing_history(user_id, 20, 0).len(), 1);
    }

    #[test]
    fn lifecycle_events_preserve_status_and_history() {
        let database = test_db();
        let user_id = "socials_existing_user";
        database.upsert_subscription(&crate::db::SubscriptionRecord {
            clerk_user_id: user_id.to_string(),
            stripe_customer_id: "cus_existing".to_string(),
            stripe_subscription_id: Some("sub_existing".to_string()),
            plan_type: "monthly".to_string(),
            status: "active".to_string(),
            trial_end: None,
            current_period_start: None,
            current_period_end: None,
        });

        apply_stripe_event(
            &database,
            &json!({
                "id": "evt_update",
                "type": "customer.subscription.updated",
                "data": { "object": {
                    "customer": "cus_existing",
                    "status": "past_due",
                    "items": { "data": [{ "price": { "lookup_key": "socials_annual" }}] },
                    "current_period_start": 1_750_000_000_i64,
                    "current_period_end": 1_760_000_000_i64
                }}
            }),
        );
        let updated = database
            .get_subscription(user_id)
            .expect("updated subscription");
        assert_eq!(updated.status, "past_due");
        assert_eq!(updated.plan_type, "annual");

        apply_stripe_event(
            &database,
            &json!({
                "id": "evt_invoice",
                "type": "invoice.paid",
                "data": { "object": {
                    "customer": "cus_existing",
                    "amount_paid": 699
                }}
            }),
        );
        apply_stripe_event(
            &database,
            &json!({
                "id": "evt_deleted",
                "type": "customer.subscription.deleted",
                "data": { "object": { "customer": "cus_existing" }}
            }),
        );

        assert_eq!(
            database
                .get_subscription(user_id)
                .expect("cancelled")
                .status,
            "cancelled"
        );
        assert_eq!(database.get_billing_history(user_id, 20, 0).len(), 2);
    }
}
