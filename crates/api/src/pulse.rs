use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::json;

use crate::agent_auth::SocialWriteAuth;
use crate::clerk::ClerkUser;
use crate::social_policy::PostAudience;
use crate::state::AppState;

type ApiResponse = (StatusCode, Json<serde_json::Value>);

fn ok(v: serde_json::Value) -> ApiResponse {
    (StatusCode::OK, Json(v))
}
fn not_found(msg: &str) -> ApiResponse {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": msg, "code": "NOT_FOUND" })),
    )
}
fn bad_request(msg: &str) -> ApiResponse {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": msg, "code": "BAD_REQUEST" })),
    )
}
fn conflict_transition(msg: &str) -> ApiResponse {
    (
        StatusCode::CONFLICT,
        Json(json!({ "error": msg, "code": "ILLEGAL_TRANSITION" })),
    )
}

fn db(state: &AppState) -> &crate::db::Database {
    state.db.as_ref().expect("database not initialized")
}

/// Pure transition matrix for Pulse draft statuses.
///
/// - `pending` → `approved` | `rejected`
/// - `approved` → `published` | `rejected`
/// - `published` → (none)
/// - `rejected` → (none; re-create a new draft instead)
pub fn allowed_pulse_transition(from: &str, to: &str) -> bool {
    matches!(
        (from, to),
        ("pending", "approved")
            | ("pending", "rejected")
            | ("approved", "published")
            | ("approved", "rejected")
    )
}

/// Expected source statuses for a target transition (for CAS WHERE clauses).
pub fn pulse_transition_expected_from(to: &str) -> &'static [&'static str] {
    match to {
        "approved" => &["pending"],
        "rejected" => &["pending", "approved"],
        "published" => &["approved"],
        _ => &[],
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PulseTransitionError {
    NotFound,
    Illegal { from: String, to: String },
}

impl PulseTransitionError {
    pub fn message(&self) -> String {
        match self {
            Self::NotFound => "Draft not found".into(),
            Self::Illegal { from, to } => {
                format!("Illegal draft transition: {from} → {to}")
            }
        }
    }
}

/// CAS status change guarded by [`allowed_pulse_transition`].
pub fn try_pulse_transition(
    database: &crate::db::Database,
    id: &str,
    profile_id: &str,
    to: &str,
) -> Result<serde_json::Value, PulseTransitionError> {
    let draft = database
        .pulse_get_draft(id, profile_id)
        .ok_or(PulseTransitionError::NotFound)?;
    let from = draft["status"].as_str().unwrap_or("").to_string();
    if !allowed_pulse_transition(&from, to) {
        return Err(PulseTransitionError::Illegal {
            from,
            to: to.to_string(),
        });
    }
    let expected = pulse_transition_expected_from(to);
    match database.pulse_cas_update_draft_status(id, profile_id, expected, to) {
        Some(updated) => Ok(updated),
        None => {
            // Race: status changed between read and CAS.
            match database.pulse_get_draft(id, profile_id) {
                None => Err(PulseTransitionError::NotFound),
                Some(current) => {
                    let cur = current["status"].as_str().unwrap_or("").to_string();
                    Err(PulseTransitionError::Illegal {
                        from: cur,
                        to: to.to_string(),
                    })
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PulsePublishError {
    NotFound,
    Illegal { status: String },
    InvalidAudience,
}

impl PulsePublishError {
    pub fn message(&self) -> String {
        match self {
            Self::NotFound => "Draft not found".into(),
            Self::Illegal { status } => {
                format!("Draft must be approved before publishing (status is '{status}')")
            }
            Self::InvalidAudience => "Draft has an invalid or unsupported audience".into(),
        }
    }
}

/// Shared publish path for HTTP + tools: CAS approved→published, then create social post.
pub fn publish_approved_pulse_draft(
    database: &crate::db::Database,
    profile_id: &str,
    id: &str,
) -> Result<(serde_json::Value, String), PulsePublishError> {
    let draft = database
        .pulse_get_draft(id, profile_id)
        .ok_or(PulsePublishError::NotFound)?;
    let status = draft["status"].as_str().unwrap_or("").to_string();
    if status != "approved" || !allowed_pulse_transition(&status, "published") {
        return Err(PulsePublishError::Illegal { status });
    }

    let body = draft["body"].as_str().unwrap_or("").to_string();
    let visibility = draft["visibility"]
        .as_str()
        .and_then(PostAudience::from_storage)
        .filter(|audience| !matches!(audience, PostAudience::Guild | PostAudience::Circle))
        .ok_or(PulsePublishError::InvalidAudience)?;
    let author_mode = draft["authorMode"].as_str().unwrap_or("person").to_string();
    let linked_agent_id = draft["linkedAgentId"].as_str().map(|s| s.to_string());

    // CAS first so concurrent publish / schedule worker cannot double-post.
    let updated = database
        .pulse_cas_update_draft_status(id, profile_id, &["approved"], "published")
        .ok_or_else(|| match database.pulse_get_draft(id, profile_id) {
            None => PulsePublishError::NotFound,
            Some(current) => PulsePublishError::Illegal {
                status: current["status"].as_str().unwrap_or("").to_string(),
            },
        })?;

    let post = database.social_create_post(
        profile_id,
        &body,
        visibility.as_str(),
        &author_mode,
        linked_agent_id.as_deref(),
        None,
        None,
        None,
    );
    let post_id = post["id"].as_str().unwrap_or("").to_string();
    database.pulse_add_audit(
        id,
        profile_id,
        "published",
        Some(&json!({ "postId": post_id }).to_string()),
    );
    Ok((updated, post_id))
}

#[derive(Debug, Deserialize)]
pub struct ListDraftsQuery {
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateDraftRequest {
    pub body: String,
    pub visibility: Option<PostAudience>,
    #[serde(rename = "authorMode")]
    pub author_mode: Option<String>,
    #[serde(rename = "linkedAgentId")]
    pub linked_agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RejectDraftRequest {
    pub reason: Option<String>,
}

pub async fn list_drafts(
    user: ClerkUser,
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListDraftsQuery>,
) -> ApiResponse {
    let profile = match db(&state).social_find_profile_by_clerk_id(&user.user_id) {
        Some(p) => p,
        None => return ok(json!({ "drafts": [] })),
    };
    let profile_id = profile["id"].as_str().unwrap_or("");
    let drafts = db(&state).pulse_list_drafts(profile_id, query.status.as_deref());
    ok(json!({ "drafts": drafts }))
}

pub async fn create_draft(
    _auth: SocialWriteAuth,
    Json(request): Json<CreateDraftRequest>,
) -> ApiResponse {
    let CreateDraftRequest {
        body,
        visibility,
        author_mode,
        linked_agent_id,
    } = request;
    let _ = (body, visibility, author_mode, linked_agent_id);
    crate::socials_billing::unavailable_response()
}

pub async fn get_draft(
    user: ClerkUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResponse {
    let profile = match db(&state).social_find_profile_by_clerk_id(&user.user_id) {
        Some(p) => p,
        None => return not_found("Not found"),
    };
    let profile_id = profile["id"].as_str().unwrap_or("");
    match db(&state).pulse_get_draft(&id, profile_id) {
        Some(draft) => ok(json!({ "draft": draft })),
        None => not_found("Draft not found"),
    }
}

pub async fn approve_draft(
    user: ClerkUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResponse {
    let profile = match db(&state).social_find_profile_by_clerk_id(&user.user_id) {
        Some(p) => p,
        None => return not_found("Not found"),
    };
    let profile_id = profile["id"].as_str().unwrap_or("");
    // Only pending → approved (CAS).
    match try_pulse_transition(db(&state), &id, profile_id, "approved") {
        Ok(draft) => {
            db(&state).pulse_add_audit(&id, profile_id, "approved", None);
            ok(json!({ "ok": true, "draft": draft }))
        }
        Err(PulseTransitionError::NotFound) => not_found("Draft not found"),
        Err(e @ PulseTransitionError::Illegal { .. }) => conflict_transition(&e.message()),
    }
}

pub async fn reject_draft(
    user: ClerkUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<RejectDraftRequest>,
) -> ApiResponse {
    let profile = match db(&state).social_find_profile_by_clerk_id(&user.user_id) {
        Some(p) => p,
        None => return not_found("Not found"),
    };
    let profile_id = profile["id"].as_str().unwrap_or("");
    // pending | approved → rejected (CAS).
    match try_pulse_transition(db(&state), &id, profile_id, "rejected") {
        Ok(draft) => {
            let details = req
                .reason
                .as_deref()
                .map(|r| json!({ "reason": r }).to_string());
            db(&state).pulse_add_audit(&id, profile_id, "rejected", details.as_deref());
            ok(json!({ "ok": true, "draft": draft }))
        }
        Err(PulseTransitionError::NotFound) => not_found("Draft not found"),
        Err(e @ PulseTransitionError::Illegal { .. }) => conflict_transition(&e.message()),
    }
}

pub async fn publish_draft(
    user: ClerkUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResponse {
    let profile = match db(&state).social_find_profile_by_clerk_id(&user.user_id) {
        Some(p) => p,
        None => return not_found("Not found"),
    };
    let profile_id = profile["id"].as_str().unwrap_or("");

    match publish_approved_pulse_draft(db(&state), profile_id, &id) {
        Ok((updated_draft, post_id)) => {
            ok(json!({ "ok": true, "draft": updated_draft, "postId": post_id }))
        }
        Err(PulsePublishError::NotFound) => not_found("Draft not found"),
        Err(e @ PulsePublishError::Illegal { .. }) => conflict_transition(&e.message()),
        Err(PulsePublishError::InvalidAudience) => {
            bad_request("Draft has an invalid or unsupported audience")
        }
    }
}

pub async fn get_draft_audit(
    user: ClerkUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResponse {
    let profile = match db(&state).social_find_profile_by_clerk_id(&user.user_id) {
        Some(p) => p,
        None => return not_found("Not found"),
    };
    let profile_id = profile["id"].as_str().unwrap_or("");
    if db(&state).pulse_get_draft(&id, profile_id).is_none() {
        return not_found("Draft not found");
    }
    let audit = db(&state).pulse_get_audit(&id);
    ok(json!({ "audit": audit }))
}

// ─── Pulse chat (tools_v1 keyword router + tools_v2 LLM when keys set) ───────

#[derive(Debug, Deserialize)]
pub struct PulseChatRequest {
    pub message: String,
    /// Optional prior messages for tools_v2 LLM context.
    #[serde(default)]
    pub history: Vec<PulseChatHistoryItem>,
}

#[derive(Debug, Deserialize)]
pub struct PulseChatHistoryItem {
    pub role: Option<String>,
    pub content: Option<String>,
}

/// Parsed tool invocation (shared by tools_v1 keywords and tools_v2 JSON).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PulseToolCall {
    pub tool: String,
    pub args: serde_json::Map<String, serde_json::Value>,
}

/// Outcome of server-side tool execution (never trusts model to mutate alone).
#[derive(Debug)]
pub(crate) struct PulseToolOutcome {
    pub reply: String,
    pub tools_used: Vec<String>,
    pub draft: Option<serde_json::Value>,
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Strip common "draft a post about …" prefixes to get the body.
pub(crate) fn extract_draft_body(message: &str) -> Option<String> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_lowercase();
    if lower.starts_with("approve ")
        || lower.starts_with("reject ")
        || lower.starts_with("publish ")
        || lower.starts_with("schedule ")
        || lower.starts_with("list ")
    {
        return None;
    }
    let prefixes = [
        "draft a post about ",
        "draft a post saying ",
        "draft a post that says ",
        "draft a post: ",
        "draft a post ",
        "write a post about ",
        "write a post saying ",
        "write a post: ",
        "write a post ",
        "create a draft about ",
        "create a draft: ",
        "create a draft ",
        "draft about ",
        "draft: ",
        "post about ",
        "post: ",
    ];
    for prefix in prefixes {
        if lower.starts_with(prefix) {
            let body = trimmed[prefix.len()..].trim();
            if body.len() >= 3 {
                return Some(body.to_string());
            }
        }
    }
    if lower.starts_with("draft ") || lower.starts_with("write ") {
        let rest = trimmed.split_once(' ').map(|(_, r)| r.trim()).unwrap_or("");
        let rest = rest
            .trim_start_matches(|c: char| c.is_ascii_whitespace())
            .trim_start_matches("a ")
            .trim_start_matches("post ")
            .trim_start_matches("about ")
            .trim_start_matches("saying ")
            .trim();
        if rest.len() >= 8 {
            return Some(rest.to_string());
        }
    }
    None
}

/// Extract a draft id from phrases like "approve draft <id>" or "approve <id>".
pub(crate) fn extract_draft_id(message: &str, verbs: &[&str]) -> Option<String> {
    let trimmed = message.trim();
    let lower = trimmed.to_lowercase();
    for verb in verbs {
        let verb_lower = verb.to_lowercase();
        if !lower.starts_with(&verb_lower) {
            continue;
        }
        let rest = trimmed[verb.len()..].trim();
        let rest = rest
            .strip_prefix("draft")
            .or_else(|| rest.strip_prefix("Draft"))
            .map(|r| r.trim())
            .unwrap_or(rest);
        let rest = rest.trim_start_matches(':').trim();
        let id = rest.split_whitespace().next().unwrap_or("").trim();
        if is_plausible_id(id) {
            return Some(id.to_string());
        }
    }
    None
}

fn is_plausible_id(id: &str) -> bool {
    let len = id.len();
    if !(8..=64).contains(&len) {
        return false;
    }
    id.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Parse "schedule draft <id> at <time>" / "schedule draft <id> for <time>".
pub(crate) fn extract_schedule_command(message: &str) -> Option<(String, String)> {
    let trimmed = message.trim();
    let lower = trimmed.to_lowercase();
    let prefixes = ["schedule draft ", "schedule "];
    let mut rest = None;
    for p in prefixes {
        if lower.starts_with(p) {
            rest = Some(trimmed[p.len()..].trim());
            break;
        }
    }
    let rest = rest?;
    let rest = rest
        .strip_prefix("draft ")
        .or_else(|| rest.strip_prefix("Draft "))
        .unwrap_or(rest)
        .trim();
    let id = rest.split_whitespace().next()?.trim();
    if !is_plausible_id(id) {
        return None;
    }
    let after_id = rest[id.len()..].trim();
    let after_id = after_id
        .strip_prefix("at ")
        .or_else(|| after_id.strip_prefix("At "))
        .or_else(|| after_id.strip_prefix("for "))
        .or_else(|| after_id.strip_prefix("For "))
        .or_else(|| after_id.strip_prefix("on "))
        .or_else(|| after_id.strip_prefix("On "))
        .unwrap_or(after_id)
        .trim();
    if after_id.is_empty() {
        return None;
    }
    let publish_at = parse_publish_at(after_id)?;
    Some((id.to_string(), publish_at))
}

/// Parse ISO-8601 primarily; simple relative times optional (in N hours/minutes/days).
pub(crate) fn parse_publish_at(raw: &str) -> Option<String> {
    let s = raw.trim().trim_matches(|c| c == '"' || c == '\'');
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(
            dt.with_timezone(&chrono::Utc)
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string(),
        );
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(naive.and_utc().format("%Y-%m-%dT%H:%M:%SZ").to_string());
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(naive.and_utc().format("%Y-%m-%dT%H:%M:%SZ").to_string());
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M") {
        return Some(naive.and_utc().format("%Y-%m-%dT%H:%M:%SZ").to_string());
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(
            date.and_hms_opt(0, 0, 0)?
                .and_utc()
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string(),
        );
    }

    let lower = s.to_lowercase();
    if let Some(rest) = lower.strip_prefix("in ") {
        let mut parts = rest.split_whitespace();
        let n: i64 = parts.next()?.parse().ok()?;
        if n <= 0 || n > 365 * 24 {
            return None;
        }
        let unit = parts.next().unwrap_or("");
        let duration = match unit {
            "minute" | "minutes" | "min" | "mins" | "m" => chrono::Duration::minutes(n),
            "hour" | "hours" | "hr" | "hrs" | "h" => chrono::Duration::hours(n),
            "day" | "days" | "d" => chrono::Duration::days(n),
            _ => return None,
        };
        let when = chrono::Utc::now() + duration;
        return Some(when.format("%Y-%m-%dT%H:%M:%SZ").to_string());
    }
    if lower == "tomorrow" {
        let when = chrono::Utc::now() + chrono::Duration::days(1);
        return Some(when.format("%Y-%m-%dT%H:%M:%SZ").to_string());
    }
    None
}

const TOOLS_V1_HELP: &str = "I'm Pulse draft tools (v1) — keyword matching, no LLM.\n\n\
• \"draft a post about …\" — create a draft\n\
• \"list drafts\" — show pending drafts\n\
• \"approve draft <id>\" / \"approve <id>\" — approve a draft\n\
• \"reject draft <id>\" / \"reject <id>\" — reject a draft\n\
• \"publish draft <id>\" — publish an approved draft\n\
• \"schedule draft <id> at <ISO time>\" — schedule an approved draft\n\
• \"list my posts\" / \"my posts\" — show your recent posts\n\n\
I use the same server mutations as the Drafts API. I won't post publicly without approval.\n\
When ANTHROPIC_API_KEY or OPENAI_API_KEY is set, the server may use tools_v2 (LLM tool JSON).";

const TOOLS_V2_SYSTEM: &str =
    "You are Pulse, a draft assistant for HeyVera. You do NOT publish posts yourself.\n\
Reply with ONLY JSON (no markdown fences, no prose outside JSON).\n\n\
Single tool:\n\
{\"tool\":\"create_draft\",\"args\":{\"body\":\"...\"}}\n\n\
Multiple tools (optional):\n\
{\"tools\":[{\"tool\":\"list_drafts\",\"args\":{}},{\"tool\":\"help\",\"args\":{}}]}\n\n\
Allowed tools ONLY:\n\
- create_draft: args.body (string, post text)\n\
- list_drafts: args.status optional (pending|approved|rejected|published)\n\
- approve_draft: args.id (draft id)\n\
- reject_draft: args.id, args.reason optional\n\
- publish_draft: args.id (draft must already be approved server-side)\n\
- list_my_posts: no required args\n\
- schedule_draft: args.id, args.publish_at (ISO-8601 UTC preferred)\n\
- help: no args\n\n\
Rules:\n\
- Never invent draft ids. If the user did not give an id, use list_drafts first or ask via help.\n\
- publish_draft and schedule_draft require approved drafts; the server enforces this.\n\
- Prefer concise body text for create_draft.\n";

fn outcome_to_json(mode: &str, o: PulseToolOutcome) -> serde_json::Value {
    let mut v = json!({
        "reply": o.reply,
        "mode": mode,
        "toolsUsed": o.tools_used,
        "draft": o.draft,
    });
    if let Some(obj) = v.as_object_mut() {
        for (k, val) in o.extra {
            obj.insert(k, val);
        }
    }
    v
}

/// Execute one tool against the database. All mutations go through existing DB helpers.
pub(crate) fn execute_pulse_tool(
    database: &crate::db::Database,
    profile_id: &str,
    call: &PulseToolCall,
) -> PulseToolOutcome {
    let tool = call.tool.as_str();
    match tool {
        "help" => PulseToolOutcome {
            reply: TOOLS_V1_HELP.to_string(),
            tools_used: vec!["help".into()],
            draft: None,
            extra: serde_json::Map::new(),
        },
        "create_draft" => {
            let mut extra = serde_json::Map::new();
            extra.insert(
                "code".into(),
                json!(crate::socials_billing::SOCIALS_BILLING_UNAVAILABLE_CODE),
            );
            PulseToolOutcome {
                reply: crate::socials_billing::SOCIALS_BILLING_UNAVAILABLE_MESSAGE.into(),
                tools_used: vec!["create_draft".into()],
                draft: None,
                extra,
            }
        }
        "list_drafts" => {
            let status = call
                .args
                .get("status")
                .and_then(|s| s.as_str())
                .filter(|s| !s.is_empty());
            let status = status.or(Some("pending"));
            let drafts = database.pulse_list_drafts(profile_id, status);
            let count = drafts.len();
            let preview: Vec<String> = drafts
                .iter()
                .take(5)
                .filter_map(|d| {
                    let body = d["body"].as_str().unwrap_or("");
                    let id = d["id"].as_str().unwrap_or("");
                    let st = d["status"].as_str().unwrap_or("");
                    if body.is_empty() {
                        None
                    } else {
                        let short = if body.len() > 60 {
                            format!("{}…", &body[..60])
                        } else {
                            body.to_string()
                        };
                        Some(format!("• [{st}] {short} ({id})"))
                    }
                })
                .collect();
            let reply = if count == 0 {
                format!(
                    "You have no {} drafts. Try: \"draft a post about …\"",
                    status.unwrap_or("matching")
                )
            } else {
                format!(
                    "You have {count} draft(s). Approve with \"approve draft <id>\", then publish or schedule.\n{}",
                    preview.join("\n")
                )
            };
            let mut extra = serde_json::Map::new();
            extra.insert("draftCount".into(), json!(count));
            PulseToolOutcome {
                reply,
                tools_used: vec!["list_drafts".into()],
                draft: None,
                extra,
            }
        }
        "approve_draft" => {
            let id = call
                .args
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if !is_plausible_id(id) {
                return PulseToolOutcome {
                    reply: "approve_draft needs a valid draft id.".into(),
                    tools_used: vec!["approve_draft".into()],
                    draft: None,
                    extra: serde_json::Map::new(),
                };
            }
            match try_pulse_transition(database, id, profile_id, "approved") {
                Ok(draft) => {
                    database.pulse_add_audit(id, profile_id, "approved", None);
                    PulseToolOutcome {
                        reply: format!(
                            "Approved draft {id}. You can publish it with \"publish draft {id}\" or schedule it."
                        ),
                        tools_used: vec!["approve_draft".into()],
                        draft: Some(draft),
                        extra: serde_json::Map::new(),
                    }
                }
                Err(PulseTransitionError::NotFound) => PulseToolOutcome {
                    reply: format!("I couldn't find draft {id} on your profile."),
                    tools_used: vec!["approve_draft".into()],
                    draft: None,
                    extra: serde_json::Map::new(),
                },
                Err(e @ PulseTransitionError::Illegal { .. }) => {
                    let mut extra = serde_json::Map::new();
                    extra.insert("code".into(), json!("ILLEGAL_TRANSITION"));
                    PulseToolOutcome {
                        reply: format!(
                            "Cannot approve draft {id}: {}. Only pending drafts can be approved.",
                            e.message()
                        ),
                        tools_used: vec!["approve_draft".into()],
                        draft: database.pulse_get_draft(id, profile_id),
                        extra,
                    }
                }
            }
        }
        "reject_draft" => {
            let id = call
                .args
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if !is_plausible_id(id) {
                return PulseToolOutcome {
                    reply: "reject_draft needs a valid draft id.".into(),
                    tools_used: vec!["reject_draft".into()],
                    draft: None,
                    extra: serde_json::Map::new(),
                };
            }
            match try_pulse_transition(database, id, profile_id, "rejected") {
                Ok(draft) => {
                    let reason = call.args.get("reason").and_then(|r| r.as_str());
                    let details = reason.map(|r| json!({ "reason": r }).to_string());
                    database.pulse_add_audit(id, profile_id, "rejected", details.as_deref());
                    PulseToolOutcome {
                        reply: format!("Rejected draft {id}."),
                        tools_used: vec!["reject_draft".into()],
                        draft: Some(draft),
                        extra: serde_json::Map::new(),
                    }
                }
                Err(PulseTransitionError::NotFound) => PulseToolOutcome {
                    reply: format!("I couldn't find draft {id} on your profile."),
                    tools_used: vec!["reject_draft".into()],
                    draft: None,
                    extra: serde_json::Map::new(),
                },
                Err(e @ PulseTransitionError::Illegal { .. }) => {
                    let mut extra = serde_json::Map::new();
                    extra.insert("code".into(), json!("ILLEGAL_TRANSITION"));
                    PulseToolOutcome {
                        reply: format!(
                            "Cannot reject draft {id}: {}. Only pending or approved drafts can be rejected.",
                            e.message()
                        ),
                        tools_used: vec!["reject_draft".into()],
                        draft: database.pulse_get_draft(id, profile_id),
                        extra,
                    }
                }
            }
        }
        "publish_draft" => {
            let id = call
                .args
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if !is_plausible_id(id) {
                return PulseToolOutcome {
                    reply: "publish_draft needs a valid draft id.".into(),
                    tools_used: vec!["publish_draft".into()],
                    draft: None,
                    extra: serde_json::Map::new(),
                };
            }
            match publish_approved_pulse_draft(database, profile_id, id) {
                Ok((updated_draft, post_id)) => {
                    let mut extra = serde_json::Map::new();
                    extra.insert("postId".into(), json!(post_id));
                    PulseToolOutcome {
                        reply: format!("Published draft {id} as post {post_id}."),
                        tools_used: vec!["publish_draft".into()],
                        draft: Some(updated_draft),
                        extra,
                    }
                }
                Err(PulsePublishError::NotFound) => PulseToolOutcome {
                    reply: format!("I couldn't find draft {id} on your profile."),
                    tools_used: vec!["publish_draft".into()],
                    draft: None,
                    extra: serde_json::Map::new(),
                },
                Err(e @ PulsePublishError::Illegal { .. }) => {
                    let mut extra = serde_json::Map::new();
                    extra.insert("code".into(), json!("ILLEGAL_TRANSITION"));
                    PulseToolOutcome {
                        reply: format!(
                            "{}. Try \"approve draft {id}\" first.",
                            e.message()
                        ),
                        tools_used: vec!["publish_draft".into()],
                        draft: database.pulse_get_draft(id, profile_id),
                        extra,
                    }
                }
                Err(PulsePublishError::InvalidAudience) => PulseToolOutcome {
                    reply: "That draft has an invalid or unsupported audience. Create a new draft with a supported audience.".into(),
                    tools_used: vec!["publish_draft".into()],
                    draft: database.pulse_get_draft(id, profile_id),
                    extra: serde_json::Map::new(),
                },
            }
        }
        "list_my_posts" => {
            let posts = database.social_get_user_posts(profile_id, 10, None, None);
            let count = posts.len();
            let preview: Vec<String> = posts
                .iter()
                .take(5)
                .filter_map(|p| {
                    let body = p["body"].as_str().unwrap_or("");
                    let id = p["id"].as_str().unwrap_or("");
                    if body.is_empty() {
                        None
                    } else {
                        let short = if body.len() > 60 {
                            format!("{}…", &body[..60])
                        } else {
                            body.to_string()
                        };
                        Some(format!("• {short} ({id})"))
                    }
                })
                .collect();
            let reply = if count == 0 {
                "You have no public posts yet. Draft something, approve it, then publish.".into()
            } else {
                format!(
                    "Your {count} most recent public post(s):\n{}",
                    preview.join("\n")
                )
            };
            let mut extra = serde_json::Map::new();
            extra.insert("postCount".into(), json!(count));
            extra.insert("posts".into(), json!(posts));
            PulseToolOutcome {
                reply,
                tools_used: vec!["list_my_posts".into()],
                draft: None,
                extra,
            }
        }
        "schedule_draft" => {
            let id = call
                .args
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let publish_at_raw = call
                .args
                .get("publish_at")
                .or_else(|| call.args.get("publishAt"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if !is_plausible_id(id) {
                return PulseToolOutcome {
                    reply: "schedule_draft needs a valid draft id.".into(),
                    tools_used: vec!["schedule_draft".into()],
                    draft: None,
                    extra: serde_json::Map::new(),
                };
            }
            let publish_at = match parse_publish_at(publish_at_raw) {
                Some(t) => t,
                None => {
                    return PulseToolOutcome {
                        reply: format!(
                            "Could not parse publish time \"{publish_at_raw}\". Use ISO-8601 like 2026-07-17T15:00:00Z or \"in 2 hours\"."
                        ),
                        tools_used: vec!["schedule_draft".into()],
                        draft: None,
                        extra: serde_json::Map::new(),
                    };
                }
            };
            match database.pulse_schedule_draft(profile_id, id, &publish_at) {
                Some(sched) => {
                    database.pulse_add_audit(
                        id,
                        profile_id,
                        "scheduled",
                        Some(&json!({ "publishAt": publish_at }).to_string()),
                    );
                    let mut extra = serde_json::Map::new();
                    extra.insert("schedule".into(), sched.clone());
                    PulseToolOutcome {
                        reply: format!(
                            "Scheduled draft {id} for {publish_at}. Due schedules are processed by POST /v1/pulse/schedules/process (cron or self-call)."
                        ),
                        tools_used: vec!["schedule_draft".into()],
                        draft: database.pulse_get_draft(id, profile_id),
                        extra,
                    }
                }
                None => PulseToolOutcome {
                    reply: format!(
                        "Could not schedule draft {id}. It must exist, be yours, and be approved first."
                    ),
                    tools_used: vec!["schedule_draft".into()],
                    draft: database.pulse_get_draft(id, profile_id),
                    extra: serde_json::Map::new(),
                },
            }
        }
        other => PulseToolOutcome {
            reply: format!("Unknown tool \"{other}\". Try \"help\"."),
            tools_used: vec![],
            draft: None,
            extra: serde_json::Map::new(),
        },
    }
}

fn merge_outcomes(outcomes: Vec<PulseToolOutcome>) -> PulseToolOutcome {
    if outcomes.is_empty() {
        return PulseToolOutcome {
            reply: "No tools ran.".into(),
            tools_used: vec![],
            draft: None,
            extra: serde_json::Map::new(),
        };
    }
    if outcomes.len() == 1 {
        return outcomes.into_iter().next().unwrap();
    }
    let mut reply_parts = Vec::new();
    let mut tools_used = Vec::new();
    let mut draft = None;
    let mut extra = serde_json::Map::new();
    for o in outcomes {
        reply_parts.push(o.reply);
        tools_used.extend(o.tools_used);
        if o.draft.is_some() {
            draft = o.draft;
        }
        for (k, v) in o.extra {
            extra.insert(k, v);
        }
    }
    PulseToolOutcome {
        reply: reply_parts.join("\n\n"),
        tools_used,
        draft,
        extra,
    }
}

/// Keyword / phrase router (tools_v1).
pub(crate) fn match_tools_v1(message: &str) -> Option<PulseToolCall> {
    let lower = message.to_lowercase();

    if lower.contains("list my posts")
        || lower == "my posts"
        || lower.contains("show my posts")
        || lower.contains("my recent posts")
    {
        return Some(PulseToolCall {
            tool: "list_my_posts".into(),
            args: serde_json::Map::new(),
        });
    }

    if let Some((id, publish_at)) = extract_schedule_command(message) {
        let mut args = serde_json::Map::new();
        args.insert("id".into(), json!(id));
        args.insert("publish_at".into(), json!(publish_at));
        return Some(PulseToolCall {
            tool: "schedule_draft".into(),
            args,
        });
    }

    if let Some(id) = extract_draft_id(message, &["approve draft", "approve"]) {
        let mut args = serde_json::Map::new();
        args.insert("id".into(), json!(id));
        return Some(PulseToolCall {
            tool: "approve_draft".into(),
            args,
        });
    }

    if let Some(id) = extract_draft_id(message, &["reject draft", "reject"]) {
        let mut args = serde_json::Map::new();
        args.insert("id".into(), json!(id));
        return Some(PulseToolCall {
            tool: "reject_draft".into(),
            args,
        });
    }

    if let Some(id) = extract_draft_id(message, &["publish draft", "publish"]) {
        let mut args = serde_json::Map::new();
        args.insert("id".into(), json!(id));
        return Some(PulseToolCall {
            tool: "publish_draft".into(),
            args,
        });
    }

    if lower.contains("list draft")
        || lower.contains("show draft")
        || lower.contains("my draft")
        || lower == "drafts"
        || lower.contains("pending draft")
    {
        return Some(PulseToolCall {
            tool: "list_drafts".into(),
            args: serde_json::Map::new(),
        });
    }

    if lower.contains("help") || lower.contains("what can") || lower == "?" {
        return Some(PulseToolCall {
            tool: "help".into(),
            args: serde_json::Map::new(),
        });
    }

    if let Some(body) = extract_draft_body(message) {
        let mut args = serde_json::Map::new();
        args.insert("body".into(), json!(body));
        return Some(PulseToolCall {
            tool: "create_draft".into(),
            args,
        });
    }

    None
}

fn tools_v1_default_outcome() -> PulseToolOutcome {
    PulseToolOutcome {
        reply: "I can create, list, approve, reject, publish, and schedule drafts for your HeyVera profile (same mutations as the Drafts API). Try \"help\", \"draft a post about …\", \"list drafts\", \"schedule draft <id> at 2026-07-18T12:00:00Z\", or \"list my posts\". Without LLM API keys this is the server-side tools_v1 path.".into(),
        tools_used: vec![],
        draft: None,
        extra: serde_json::Map::new(),
    }
}

/// Run tools_v1 keyword matching against the message.
pub(crate) fn run_tools_v1(
    database: &crate::db::Database,
    profile_id: &str,
    message: &str,
) -> PulseToolOutcome {
    match match_tools_v1(message) {
        Some(call) => execute_pulse_tool(database, profile_id, &call),
        None => tools_v1_default_outcome(),
    }
}

/// Conservatively extract tool calls from LLM text (JSON object or array).
pub(crate) fn parse_tool_calls_from_llm(text: &str) -> Result<Vec<PulseToolCall>, String> {
    let trimmed = text.trim();
    let stripped = if let Some(s) = trimmed.strip_prefix("```json") {
        s.trim_end_matches("```").trim()
    } else if let Some(s) = trimmed.strip_prefix("```") {
        s.trim_end_matches("```").trim()
    } else {
        trimmed
    };

    let json_slice = extract_json_blob(stripped).ok_or_else(|| "no JSON found".to_string())?;
    let value: serde_json::Value =
        serde_json::from_str(json_slice).map_err(|e| format!("JSON parse error: {e}"))?;

    let mut calls = Vec::new();

    if let Some(arr) = value.get("tools").and_then(|t| t.as_array()) {
        for item in arr {
            if let Some(c) = value_to_tool_call(item) {
                calls.push(c);
            }
        }
    } else if let Some(c) = value_to_tool_call(&value) {
        calls.push(c);
    } else if let Some(arr) = value.as_array() {
        for item in arr {
            if let Some(c) = value_to_tool_call(item) {
                calls.push(c);
            }
        }
    }

    if calls.is_empty() {
        return Err("no recognized tool calls in JSON".into());
    }

    const ALLOWED: &[&str] = &[
        "create_draft",
        "list_drafts",
        "approve_draft",
        "reject_draft",
        "publish_draft",
        "list_my_posts",
        "schedule_draft",
        "help",
    ];
    for c in &calls {
        if !ALLOWED.contains(&c.tool.as_str()) {
            return Err(format!("disallowed tool: {}", c.tool));
        }
    }
    Ok(calls)
}

fn value_to_tool_call(value: &serde_json::Value) -> Option<PulseToolCall> {
    let tool = value
        .get("tool")
        .or_else(|| value.get("name"))
        .and_then(|t| t.as_str())?
        .trim()
        .to_string();
    if tool.is_empty() {
        return None;
    }
    let args = value
        .get("args")
        .or_else(|| value.get("arguments"))
        .or_else(|| value.get("parameters"))
        .and_then(|a| a.as_object())
        .cloned()
        .unwrap_or_default();
    Some(PulseToolCall { tool, args })
}

fn extract_json_blob(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let start_obj = s.find('{');
    let start_arr = s.find('[');
    let start = match (start_obj, start_arr) {
        (Some(o), Some(a)) => o.min(a),
        (Some(o), None) => o,
        (None, Some(a)) => a,
        (None, None) => return None,
    };
    let open = bytes[start];
    let close = if open == b'{' { b'}' } else { b']' };
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b if b == open => depth += 1,
            b if b == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Pulse chat: tools_v2 when LLM keys present (with honest tools_v1 fallback); else tools_v1 only.
pub async fn pulse_chat(
    user: ClerkUser,
    State(state): State<Arc<AppState>>,
    Json(req): Json<PulseChatRequest>,
) -> ApiResponse {
    let message = req.message.trim();
    if message.is_empty() {
        return bad_request("message is required");
    }
    if message.len() > 4000 {
        return bad_request("message too long");
    }

    let profile = match db(&state).social_find_profile_by_clerk_id(&user.user_id) {
        Some(p) => p,
        None => {
            return ok(json!({
                "reply": "Create a HeyVera profile first, then I can save drafts to your account.",
                "mode": "tools_v1",
                "toolsUsed": [],
                "draft": null,
            }));
        }
    };
    let profile_id = profile["id"].as_str().unwrap_or("").to_string();
    let database = db(&state);

    if let Some((provider, api_key)) = crate::llm_client::resolve_server_llm() {
        match try_tools_v2(
            database,
            &profile_id,
            message,
            &req.history,
            provider,
            &api_key,
        )
        .await
        {
            Ok(outcome) => return ok(outcome_to_json("tools_v2", outcome)),
            Err(err) => {
                tracing::warn!(%err, "tools_v2 failed; falling back to tools_v1");
            }
        }
    }

    let outcome = run_tools_v1(database, &profile_id, message);
    ok(outcome_to_json("tools_v1", outcome))
}

async fn try_tools_v2(
    database: &crate::db::Database,
    profile_id: &str,
    message: &str,
    history: &[PulseChatHistoryItem],
    provider: crate::llm_client::Provider,
    api_key: &str,
) -> Result<PulseToolOutcome, String> {
    let mut messages: Vec<crate::llm_client::ChatMessage> = Vec::new();
    for h in history.iter().take(12) {
        let role = h.role.as_deref().unwrap_or("user");
        let content = h.content.as_deref().unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let role = if role == "assistant" || role == "vera" {
            "assistant"
        } else {
            "user"
        };
        messages.push(crate::llm_client::ChatMessage {
            role: role.into(),
            content: content.into(),
        });
    }
    messages.push(crate::llm_client::ChatMessage {
        role: "user".into(),
        content: message.into(),
    });

    let text =
        crate::llm_client::chat_completion(&provider, api_key, None, TOOLS_V2_SYSTEM, &messages)
            .await?;

    let calls = parse_tool_calls_from_llm(&text)?;
    let outcomes: Vec<PulseToolOutcome> = calls
        .iter()
        .map(|c| execute_pulse_tool(database, profile_id, c))
        .collect();
    Ok(merge_outcomes(outcomes))
}

#[derive(Debug, Deserialize)]
pub struct ScheduleDraftRequest {
    #[serde(rename = "draftId")]
    pub draft_id: String,
    /// ISO-8601 time when the approved draft should publish.
    #[serde(rename = "publishAt")]
    pub publish_at: String,
}

/// POST /v1/pulse/schedules — schedule an **approved** draft for later publish.
pub async fn schedule_draft(
    user: ClerkUser,
    State(state): State<Arc<AppState>>,
    Json(req): Json<ScheduleDraftRequest>,
) -> ApiResponse {
    let profile = match db(&state).social_find_profile_by_clerk_id(&user.user_id) {
        Some(p) => p,
        None => return not_found("Create a profile first"),
    };
    let profile_id = profile["id"].as_str().unwrap_or("");
    if req.draft_id.trim().is_empty() || req.publish_at.trim().is_empty() {
        return bad_request("draftId and publishAt are required");
    }
    match db(&state).pulse_schedule_draft(profile_id, req.draft_id.trim(), req.publish_at.trim()) {
        Some(sched) => {
            db(&state).pulse_add_audit(
                req.draft_id.trim(),
                profile_id,
                "scheduled",
                Some(&json!({ "publishAt": req.publish_at }).to_string()),
            );
            ok(json!({ "ok": true, "schedule": sched }))
        }
        None => bad_request("Draft must exist, be owned by you, and be approved before scheduling"),
    }
}

/// GET /v1/pulse/schedules
pub async fn list_schedules(user: ClerkUser, State(state): State<Arc<AppState>>) -> ApiResponse {
    let profile = match db(&state).social_find_profile_by_clerk_id(&user.user_id) {
        Some(p) => p,
        None => return ok(json!({ "schedules": [] })),
    };
    let profile_id = profile["id"].as_str().unwrap_or("");
    let schedules = db(&state).pulse_list_schedules(profile_id);
    ok(json!({ "schedules": schedules }))
}

/// POST /v1/pulse/schedules/process — process due schedules (admin/cron or owner self-call).
pub async fn process_due_schedules(
    user: ClerkUser,
    State(state): State<Arc<AppState>>,
) -> ApiResponse {
    // Authenticated; processes global due queue (single-tenant SQLite early product).
    let _ = user;
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let published = db(&state).pulse_process_due_schedules(&now);
    ok(json!({ "ok": true, "published": published }))
}

// ─── Goal plan MVP (deterministic; not Temporal) ────────────────────────────

/// A single planned step. Tools map only to existing Pulse/social capabilities.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GoalStep {
    /// One of: `create_draft`, `approve_required`, `schedule_optional`
    pub tool: String,
    pub args: serde_json::Value,
    pub description: String,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateGoalRequest {
    pub goal: String,
}

/// Extract a draft body topic from a free-form goal string.
fn extract_goal_topic(goal: &str) -> String {
    let trimmed = goal.trim();
    if trimmed.is_empty() {
        return "Untitled draft".to_string();
    }
    let lower = trimmed.to_lowercase();
    let prefixes = [
        "post about ",
        "write about ",
        "draft about ",
        "draft a post about ",
        "write a post about ",
        "create a post about ",
        "schedule a post about ",
        "post: ",
        "draft: ",
    ];
    let mut body = trimmed.to_string();
    for prefix in prefixes {
        if lower.starts_with(prefix) {
            body = trimmed[prefix.len()..].trim().to_string();
            break;
        }
    }
    // Strip common trailing schedule phrases so the draft body stays topical.
    let schedule_suffixes = [
        " next week",
        " this week",
        " tomorrow",
        " later today",
        " later",
        " tonight",
        " on monday",
        " on tuesday",
        " on wednesday",
        " on thursday",
        " on friday",
        " on saturday",
        " on sunday",
    ];
    let body_lower = body.to_lowercase();
    for suffix in schedule_suffixes {
        if body_lower.ends_with(suffix) {
            body = body[..body.len() - suffix.len()].trim().to_string();
            break;
        }
    }
    if body.is_empty() {
        "Untitled draft".to_string()
    } else {
        body
    }
}

fn goal_wants_schedule(goal: &str) -> bool {
    let lower = goal.to_lowercase();
    const MARKERS: &[&str] = &[
        "next week",
        "this week",
        "tomorrow",
        "later",
        "tonight",
        "schedule",
        "scheduled",
        " on monday",
        " on tuesday",
        " on wednesday",
        " on thursday",
        " on friday",
        " on saturday",
        " on sunday",
        "next month",
        "in a week",
        "in two weeks",
    ];
    MARKERS.iter().any(|m| lower.contains(m))
}

/// Pure deterministic plan for a natural-language goal.
/// Maps only to existing tools: create_draft, approve_required, schedule_optional.
/// Not a Temporal workflow — a plan template the user can follow via Drafts/Schedule UI.
pub fn decompose_goal(goal: &str) -> Vec<GoalStep> {
    let topic = extract_goal_topic(goal);
    let draft_body = format!("Draft about {topic}");
    let mut steps = vec![
        GoalStep {
            tool: "create_draft".to_string(),
            args: json!({ "body": draft_body }),
            description: format!("Create a draft about {topic}"),
            status: "pending".to_string(),
        },
        GoalStep {
            tool: "approve_required".to_string(),
            args: json!({}),
            description: "Human must approve before any public publish".to_string(),
            status: "pending".to_string(),
        },
    ];
    if goal_wants_schedule(goal) {
        steps.push(GoalStep {
            tool: "schedule_optional".to_string(),
            args: json!({ "hint": "when_ready" }),
            description: "Schedule the approved draft when a publish time is ready".to_string(),
            status: "pending".to_string(),
        });
    }
    steps
}

/// POST /v1/pulse/goals — create a goal and persist a deterministic plan.
pub async fn create_goal(
    user: ClerkUser,
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateGoalRequest>,
) -> ApiResponse {
    let goal = req.goal.trim();
    if goal.is_empty() {
        return bad_request("goal is required");
    }
    if goal.len() > 2000 {
        return bad_request("goal too long");
    }
    let profile = match db(&state).social_find_profile_by_clerk_id(&user.user_id) {
        Some(p) => p,
        None => return not_found("Create a profile first"),
    };
    let profile_id = profile["id"].as_str().unwrap_or("");
    let steps = decompose_goal(goal);
    let plan = json!({
        "goal": goal,
        "runtime": "deterministic_mvp",
        "note": "Plan template only — not a Temporal workflow. Execute steps via Drafts + Schedule UI.",
        "tools": ["create_draft", "approve_required", "schedule_optional"],
    });
    let plan_json = plan.to_string();
    let steps_json = serde_json::to_string(&steps).unwrap_or_else(|_| "[]".to_string());
    let row = db(&state).pulse_create_goal(profile_id, goal, &plan_json, &steps_json);
    ok(json!({ "ok": true, "goal": row }))
}

/// GET /v1/pulse/goals — list goals for the caller's profile.
pub async fn list_goals(user: ClerkUser, State(state): State<Arc<AppState>>) -> ApiResponse {
    let profile = match db(&state).social_find_profile_by_clerk_id(&user.user_id) {
        Some(p) => p,
        None => return ok(json!({ "goals": [] })),
    };
    let profile_id = profile["id"].as_str().unwrap_or("");
    let goals = db(&state).pulse_list_goals(profile_id);
    ok(json!({ "goals": goals }))
}

/// GET /v1/pulse/goals/{id}
pub async fn get_goal(
    user: ClerkUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResponse {
    let profile = match db(&state).social_find_profile_by_clerk_id(&user.user_id) {
        Some(p) => p,
        None => return not_found("Not found"),
    };
    let profile_id = profile["id"].as_str().unwrap_or("");
    match db(&state).pulse_get_goal(&id, profile_id) {
        Some(goal) => ok(json!({ "goal": goal })),
        None => not_found("Goal not found"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn test_db() -> Database {
        let dir = tempfile::tempdir().unwrap().keep();
        Database::open(&dir.join("cortex.sqlite"))
    }

    #[test]
    fn extract_draft_body_strips_common_prefixes() {
        assert_eq!(
            extract_draft_body("draft a post about shipping tools"),
            Some("shipping tools".to_string())
        );
        assert_eq!(
            extract_draft_body("write a post: hello world friends"),
            Some("hello world friends".to_string())
        );
        assert!(extract_draft_body("approve draft abc-123").is_none());
        assert!(extract_draft_body("schedule draft abc-12345 at 2026-01-01").is_none());
        assert!(extract_draft_body("list my posts").is_none());
        assert!(extract_draft_body("").is_none());
    }

    #[test]
    fn extract_draft_id_parses_approve_reject_publish() {
        assert_eq!(
            extract_draft_id("approve draft abc-123-def", &["approve draft", "approve"]),
            Some("abc-123-def".to_string())
        );
        assert_eq!(
            extract_draft_id(
                "approve 01234567-89ab-cdef-0123-456789abcdef",
                &["approve draft", "approve"]
            ),
            Some("01234567-89ab-cdef-0123-456789abcdef".to_string())
        );
        assert_eq!(
            extract_draft_id("reject draft short", &["reject draft", "reject"]),
            None
        );
        assert_eq!(
            extract_draft_id("publish draft draft_id_01", &["publish draft", "publish"]),
            Some("draft_id_01".to_string())
        );
    }

    #[test]
    fn parse_publish_at_iso_and_relative() {
        assert_eq!(
            parse_publish_at("2026-07-17T15:30:00Z").as_deref(),
            Some("2026-07-17T15:30:00Z")
        );
        assert_eq!(
            parse_publish_at("2026-07-17T15:30:00").as_deref(),
            Some("2026-07-17T15:30:00Z")
        );
        assert_eq!(
            parse_publish_at("2026-07-17").as_deref(),
            Some("2026-07-17T00:00:00Z")
        );
        let rel = parse_publish_at("in 2 hours").expect("relative");
        assert!(rel.ends_with('Z'));
        assert!(parse_publish_at("not-a-time").is_none());
    }

    #[test]
    fn extract_schedule_command_parses() {
        let (id, at) =
            extract_schedule_command("schedule draft abc-1234-xyz at 2026-08-01T12:00:00Z")
                .expect("schedule");
        assert_eq!(id, "abc-1234-xyz");
        assert_eq!(at, "2026-08-01T12:00:00Z");

        assert!(extract_schedule_command("schedule draft short at 2026-08-01").is_none());
        assert!(extract_schedule_command("list drafts").is_none());
    }

    #[test]
    fn parse_tool_calls_from_llm_accepts_single_and_multi() {
        let single = r#"{"tool":"create_draft","args":{"body":"hello world post"}}"#;
        let calls = parse_tool_calls_from_llm(single).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool, "create_draft");
        assert_eq!(calls[0].args["body"], "hello world post");

        let multi = r#"{"tools":[{"tool":"list_drafts","args":{}},{"tool":"help","args":{}}]}"#;
        let calls = parse_tool_calls_from_llm(multi).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].tool, "list_drafts");
        assert_eq!(calls[1].tool, "help");

        let fenced = "Sure.\n```json\n{\"tool\":\"help\",\"args\":{}}\n```\n";
        assert_eq!(parse_tool_calls_from_llm(fenced).unwrap()[0].tool, "help");

        assert!(parse_tool_calls_from_llm("no json here").is_err());
        assert!(parse_tool_calls_from_llm(r#"{"tool":"rm_rf","args":{}}"#).is_err());
    }

    #[test]
    fn publish_tool_requires_an_existing_approved_draft() {
        let db = test_db();
        let profile = db.social_create_profile("clerk_pulse_exec", "pulseexec", "Pulse Exec", "");
        let profile_id = profile["id"].as_str().unwrap();

        let draft =
            db.pulse_create_draft(profile_id, "Unit test draft body", "public", "person", None);
        let draft_id = draft["id"].as_str().unwrap().to_string();

        let mut pub_args = serde_json::Map::new();
        pub_args.insert("id".into(), json!(draft_id));
        let blocked = execute_pulse_tool(
            &db,
            profile_id,
            &PulseToolCall {
                tool: "publish_draft".into(),
                args: pub_args.clone(),
            },
        );
        assert!(blocked.reply.contains("must be approved"));

        let mut appr = serde_json::Map::new();
        appr.insert("id".into(), json!(draft_id));
        let _ = execute_pulse_tool(
            &db,
            profile_id,
            &PulseToolCall {
                tool: "approve_draft".into(),
                args: appr,
            },
        );
        let published = execute_pulse_tool(
            &db,
            profile_id,
            &PulseToolCall {
                tool: "publish_draft".into(),
                args: pub_args,
            },
        );
        assert!(published.reply.contains("Published"));
        assert!(published.extra.get("postId").is_some());
    }

    #[test]
    fn tools_v1_matchers_and_schedule_executor() {
        assert_eq!(
            match_tools_v1("list drafts").map(|c| c.tool),
            Some("list_drafts".into())
        );
        assert_eq!(
            match_tools_v1("draft a post about shipping soon").map(|c| c.tool),
            Some("create_draft".into())
        );

        let db = test_db();
        let profile = db.social_create_profile("clerk_sched", "scheduser", "Sched User", "");
        let profile_id = profile["id"].as_str().unwrap();
        let draft = db.pulse_create_draft(profile_id, "Schedule me", "public", "person", None);
        let id = draft["id"].as_str().unwrap();
        db.pulse_update_draft_status(id, profile_id, "approved")
            .unwrap();

        let msg = format!("schedule draft {id} at 2026-12-01T10:00:00Z");
        let call = match_tools_v1(&msg).expect("schedule match");
        assert_eq!(call.tool, "schedule_draft");
        let out = execute_pulse_tool(&db, profile_id, &call);
        assert_eq!(out.tools_used, vec!["schedule_draft"]);
        assert!(out.reply.contains("Scheduled"));
        assert!(out.extra.get("schedule").is_some());
    }

    #[test]
    fn allowed_pulse_transition_matrix() {
        // pending → approved | rejected
        assert!(allowed_pulse_transition("pending", "approved"));
        assert!(allowed_pulse_transition("pending", "rejected"));
        assert!(!allowed_pulse_transition("pending", "published"));
        assert!(!allowed_pulse_transition("pending", "pending"));

        // approved → published | rejected
        assert!(allowed_pulse_transition("approved", "published"));
        assert!(allowed_pulse_transition("approved", "rejected"));
        assert!(!allowed_pulse_transition("approved", "pending"));
        assert!(!allowed_pulse_transition("approved", "approved"));

        // published → none
        assert!(!allowed_pulse_transition("published", "pending"));
        assert!(!allowed_pulse_transition("published", "approved"));
        assert!(!allowed_pulse_transition("published", "rejected"));
        assert!(!allowed_pulse_transition("published", "published"));

        // rejected → none (re-create a new draft instead)
        assert!(!allowed_pulse_transition("rejected", "pending"));
        assert!(!allowed_pulse_transition("rejected", "approved"));
        assert!(!allowed_pulse_transition("rejected", "published"));
        assert!(!allowed_pulse_transition("rejected", "rejected"));

        // unknown statuses
        assert!(!allowed_pulse_transition("", "approved"));
        assert!(!allowed_pulse_transition("pending", ""));
        assert!(!allowed_pulse_transition("foo", "bar"));
    }

    #[test]
    fn try_pulse_transition_cas_enforces_matrix() {
        let db = test_db();
        let profile = db.social_create_profile("clerk_cas", "casuser", "CAS User", "");
        let profile_id = profile["id"].as_str().unwrap();
        let draft = db.pulse_create_draft(profile_id, "CAS body", "public", "person", None);
        let id = draft["id"].as_str().unwrap();

        // pending → published illegal
        assert!(matches!(
            try_pulse_transition(&db, id, profile_id, "published"),
            Err(PulseTransitionError::Illegal { .. })
        ));
        assert_eq!(
            db.pulse_get_draft(id, profile_id).unwrap()["status"],
            "pending"
        );

        // pending → approved ok
        let approved = try_pulse_transition(&db, id, profile_id, "approved").unwrap();
        assert_eq!(approved["status"], "approved");

        // approved → approved illegal
        assert!(matches!(
            try_pulse_transition(&db, id, profile_id, "approved"),
            Err(PulseTransitionError::Illegal { .. })
        ));

        // approved → published via shared publish helper
        let (published, post_id) = publish_approved_pulse_draft(&db, profile_id, id).unwrap();
        assert_eq!(published["status"], "published");
        assert!(!post_id.is_empty());

        // published → reject illegal
        assert!(matches!(
            try_pulse_transition(&db, id, profile_id, "rejected"),
            Err(PulseTransitionError::Illegal { .. })
        ));

        // missing draft
        assert_eq!(
            try_pulse_transition(&db, "does-not-exist-id", profile_id, "approved"),
            Err(PulseTransitionError::NotFound)
        );
    }

    #[test]
    fn try_pulse_transition_reject_from_pending_and_approved() {
        let db = test_db();
        let profile = db.social_create_profile("clerk_rej", "rejuser", "Rej User", "");
        let profile_id = profile["id"].as_str().unwrap();

        let d1 = db.pulse_create_draft(profile_id, "rej pending", "public", "person", None);
        let id1 = d1["id"].as_str().unwrap();
        let rejected = try_pulse_transition(&db, id1, profile_id, "rejected").unwrap();
        assert_eq!(rejected["status"], "rejected");
        // rejected is terminal
        assert!(matches!(
            try_pulse_transition(&db, id1, profile_id, "approved"),
            Err(PulseTransitionError::Illegal { .. })
        ));

        let d2 = db.pulse_create_draft(profile_id, "rej approved", "public", "person", None);
        let id2 = d2["id"].as_str().unwrap();
        try_pulse_transition(&db, id2, profile_id, "approved").unwrap();
        let rejected2 = try_pulse_transition(&db, id2, profile_id, "rejected").unwrap();
        assert_eq!(rejected2["status"], "rejected");
    }

    #[test]
    fn publish_approved_pulse_draft_requires_approved() {
        let db = test_db();
        let profile = db.social_create_profile("clerk_pub", "pubuser", "Pub User", "");
        let profile_id = profile["id"].as_str().unwrap();
        let draft = db.pulse_create_draft(profile_id, "pub body", "public", "person", None);
        let id = draft["id"].as_str().unwrap();

        assert!(matches!(
            publish_approved_pulse_draft(&db, profile_id, id),
            Err(PulsePublishError::Illegal { .. })
        ));

        try_pulse_transition(&db, id, profile_id, "approved").unwrap();
        let (draft, post_id) = publish_approved_pulse_draft(&db, profile_id, id).unwrap();
        assert_eq!(draft["status"], "published");
        assert!(!post_id.is_empty());

        // second publish fails
        assert!(matches!(
            publish_approved_pulse_draft(&db, profile_id, id),
            Err(PulsePublishError::Illegal { .. })
        ));
    }

    #[test]
    fn create_draft_tool_fails_closed_without_creating_a_draft() {
        let db = test_db();
        let profile = db.social_create_profile("clerk_paused", "paused", "Paused", "");
        let profile_id = profile["id"].as_str().unwrap();

        let mut args = serde_json::Map::new();
        args.insert("body".into(), json!("This draft must not be saved"));
        let created = execute_pulse_tool(
            &db,
            profile_id,
            &PulseToolCall {
                tool: "create_draft".into(),
                args,
            },
        );
        assert_eq!(created.tools_used, vec!["create_draft"]);
        assert!(created.draft.is_none());
        assert_eq!(
            created.reply,
            crate::socials_billing::SOCIALS_BILLING_UNAVAILABLE_MESSAGE
        );
        assert_eq!(
            created.extra.get("code").and_then(|c| c.as_str()),
            Some(crate::socials_billing::SOCIALS_BILLING_UNAVAILABLE_CODE)
        );
        assert_eq!(db.pulse_list_drafts(profile_id, None).len(), 0);
    }

    #[test]
    fn tools_v1_runs_without_llm_keys() {
        let db = test_db();
        let profile = db.social_create_profile("clerk_v1only", "v1only", "V1 Only", "");
        let profile_id = profile["id"].as_str().unwrap();
        let out = run_tools_v1(&db, profile_id, "help");
        assert!(
            out.tools_used.contains(&"help".to_string()) || out.reply.contains("Pulse draft tools")
        );
        let _ = crate::llm_client::resolve_server_llm();
    }

    #[test]
    fn pulse_create_draft_and_list_roundtrip() {
        let db = test_db();
        let profile = db.social_create_profile("clerk_pulse_test", "pulseuser", "Pulse User", "");
        let profile_id = profile["id"].as_str().unwrap();

        let draft =
            db.pulse_create_draft(profile_id, "Hello from unit test", "public", "person", None);
        assert_eq!(draft["status"], "pending");
        assert_eq!(draft["body"], "Hello from unit test");

        let listed = db.pulse_list_drafts(profile_id, Some("pending"));
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["id"], draft["id"]);

        let id = draft["id"].as_str().unwrap();
        let approved = db
            .pulse_update_draft_status(id, profile_id, "approved")
            .unwrap();
        assert_eq!(approved["status"], "approved");

        let pending = db.pulse_list_drafts(profile_id, Some("pending"));
        assert!(pending.is_empty());
    }

    #[test]
    fn decompose_goal_post_about_x_next_week() {
        let steps = decompose_goal("post about X next week");
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].tool, "create_draft");
        assert_eq!(steps[0].args["body"], "Draft about X");
        assert_eq!(steps[0].status, "pending");
        assert_eq!(steps[1].tool, "approve_required");
        assert!(steps[1]
            .args
            .as_object()
            .map(|o| o.is_empty())
            .unwrap_or(false));
        assert_eq!(steps[2].tool, "schedule_optional");
        assert_eq!(steps[2].args["hint"], "when_ready");
    }

    #[test]
    fn decompose_goal_simple_topic_no_schedule() {
        let steps = decompose_goal("post about shipping tools");
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].tool, "create_draft");
        assert_eq!(steps[0].args["body"], "Draft about shipping tools");
        assert_eq!(steps[1].tool, "approve_required");
    }

    #[test]
    fn decompose_goal_empty_falls_back() {
        let steps = decompose_goal("   ");
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].args["body"], "Draft about Untitled draft");
    }

    #[test]
    fn pulse_goal_create_get_list_roundtrip() {
        let db = test_db();
        let profile = db.social_create_profile("clerk_goal_test", "goaluser", "Goal User", "");
        let profile_id = profile["id"].as_str().unwrap();
        let steps = decompose_goal("post about cats next week");
        let plan = json!({
            "goal": "post about cats next week",
            "runtime": "deterministic_mvp",
        });
        let created = db.pulse_create_goal(
            profile_id,
            "post about cats next week",
            &plan.to_string(),
            &serde_json::to_string(&steps).unwrap(),
        );
        assert_eq!(created["status"], "active");
        assert_eq!(created["goal"], "post about cats next week");
        assert!(created["steps"].as_array().unwrap().len() >= 2);

        let id = created["id"].as_str().unwrap();
        let got = db.pulse_get_goal(id, profile_id).unwrap();
        assert_eq!(got["id"], id);

        let listed = db.pulse_list_goals(profile_id);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["id"], id);

        // Ownership isolation
        assert!(db.pulse_get_goal(id, "other-profile").is_none());
    }
}
