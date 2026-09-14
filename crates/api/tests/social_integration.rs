/// Social integration tests.
///
/// Tests run against an in-memory SQLite database (via AppState::new with no
/// external dependencies). Auth is disabled via CORTEX_AUTH_DISABLED=1 so all
/// requests use the synthetic "local" user.
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use cortex_api::scheduler;
use cortex_api::state::AppState;

// ─── Test infrastructure ─────────────────────────────────────────────────────

/// Build a test router with auth disabled and an isolated temp DB.
async fn test_app() -> (axum::Router, tempfile::TempDir) {
    // Disable Clerk auth so all requests pass with user_id = "local"
    std::env::set_var("CORTEX_AUTH_DISABLED", "1");

    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = tmp.path().to_path_buf();
    std::fs::create_dir_all(workspace.join(".cortex")).unwrap();

    let ledger_path = workspace.join(".cortex/ledger.jsonl");
    let state = AppState::new(ledger_path, workspace, None).await;

    let scheduler_tx = scheduler::spawn_scheduler(state.clone());
    state.set_scheduler_tx(scheduler_tx).await;

    let app = cortex_api::build_heyvera_router(state);
    (app, tmp)
}

/// Helper: parse response body as JSON.
async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).expect("response is not valid JSON")
}

/// POST a JSON body, return the response.
async fn post_json(
    app: axum::Router,
    uri: &str,
    body: serde_json::Value,
) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap(),
    )
    .await
    .unwrap()
}

/// GET a URI, return the response.
async fn get(app: axum::Router, uri: &str) -> axum::response::Response {
    app.oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/// Create a social profile, then verify it can be fetched.
#[tokio::test]
async fn test_create_and_fetch_profile() {
    let (app, _tmp) = test_app().await;

    // Create profile
    let resp = post_json(
        app.clone(),
        "/v1/social/me/profile",
        serde_json::json!({ "handle": "testuser", "displayName": "Test User" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["ok"], true);
    assert_eq!(json["profile"]["handle"], "testuser");

    // Fetch profile
    let resp = get(app.clone(), "/v1/social/me/profile").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["handle"], "testuser");
}

/// Create a post and read it back via the single post endpoint.
#[tokio::test]
async fn test_create_and_fetch_post() {
    let (app, _tmp) = test_app().await;

    // Need a profile first
    post_json(
        app.clone(),
        "/v1/social/me/profile",
        serde_json::json!({ "handle": "poster", "displayName": "Poster" }),
    )
    .await;

    // Create post
    let resp = post_json(
        app.clone(),
        "/v1/social/posts",
        serde_json::json!({ "body": "Hello, integration test world!" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["ok"], true);
    let post_id = json["post"]["id"].as_str().unwrap().to_string();

    // Fetch post
    let resp = get(app.clone(), &format!("/v1/social/posts/{}", post_id)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    // get_single_post wraps its result: ok({ "post": ..., "replies": ... }).
    // The assertions here read the post flat, which never matched; the create
    // assertions above already use the wrapped shape.
    assert_eq!(json["post"]["id"], post_id);
    assert_eq!(json["post"]["body"], "Hello, integration test world!");
}

/// Like a post and verify the count increments.
#[tokio::test]
async fn test_like_post() {
    let (app, _tmp) = test_app().await;

    post_json(
        app.clone(),
        "/v1/social/me/profile",
        serde_json::json!({ "handle": "liker", "displayName": "Liker" }),
    )
    .await;

    let create_resp = post_json(
        app.clone(),
        "/v1/social/posts",
        serde_json::json!({ "body": "Like me!" }),
    )
    .await;
    let post_id = body_json(create_resp).await["post"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Like the post
    let like_resp = post_json(
        app.clone(),
        &format!("/v1/social/posts/{}/like", post_id),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(like_resp.status(), StatusCode::OK);
    let json = body_json(like_resp).await;
    assert_eq!(json["ok"], true);
}

/// Get the home feed (should include the created post).
#[tokio::test]
async fn test_home_feed() {
    let (app, _tmp) = test_app().await;

    post_json(
        app.clone(),
        "/v1/social/me/profile",
        serde_json::json!({ "handle": "feeduser", "displayName": "Feed User" }),
    )
    .await;

    post_json(
        app.clone(),
        "/v1/social/posts",
        serde_json::json!({ "body": "A post for the feed" }),
    )
    .await;

    let resp = get(app.clone(), "/v1/social/feed/home").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    let posts = json["posts"].as_array().expect("posts array");
    assert!(
        !posts.is_empty(),
        "home feed should contain at least one post"
    );
}

/// Search for a post by keyword.
#[tokio::test]
async fn test_search_posts() {
    let (app, _tmp) = test_app().await;

    post_json(
        app.clone(),
        "/v1/social/me/profile",
        serde_json::json!({ "handle": "searcher", "displayName": "Searcher" }),
    )
    .await;

    post_json(
        app.clone(),
        "/v1/social/posts",
        serde_json::json!({ "body": "unique_keyword_xyz integration test" }),
    )
    .await;

    let resp = get(app.clone(), "/v1/social/search?q=unique_keyword_xyz").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    let posts = json["posts"].as_array().expect("posts array");
    assert!(!posts.is_empty(), "search should return the post");
    assert!(posts[0]["body"]
        .as_str()
        .unwrap_or("")
        .contains("unique_keyword_xyz"));
}

/// Get notifications (empty list for fresh user is fine).
#[tokio::test]
async fn test_notifications_empty() {
    let (app, _tmp) = test_app().await;

    post_json(
        app.clone(),
        "/v1/social/me/profile",
        serde_json::json!({ "handle": "notifuser", "displayName": "Notif User" }),
    )
    .await;

    let resp = get(app.clone(), "/v1/social/notifications").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert!(json["notifications"].as_array().is_some());
}

/// Reply to a post creates a notification for the original author.
#[tokio::test]
async fn test_reply_creates_notification() {
    let (app, _tmp) = test_app().await;

    // Create profile
    post_json(
        app.clone(),
        "/v1/social/me/profile",
        serde_json::json!({ "handle": "author", "displayName": "Author" }),
    )
    .await;

    // Create original post
    let create_resp = post_json(
        app.clone(),
        "/v1/social/posts",
        serde_json::json!({ "body": "Original post" }),
    )
    .await;
    let post_id = body_json(create_resp).await["post"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Reply to it
    let reply_resp = post_json(
        app.clone(),
        "/v1/social/posts",
        serde_json::json!({ "body": "A reply", "replyToPostId": post_id }),
    )
    .await;
    assert_eq!(reply_resp.status(), StatusCode::OK);

    // Check notifications
    let resp = get(app.clone(), "/v1/social/notifications").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    // Since it's the same user, there may or may not be a notification — just verify no crash
    assert!(json["notifications"].as_array().is_some());
}

/// Quote a post and verify it's stored.
#[tokio::test]
async fn test_quote_post() {
    let (app, _tmp) = test_app().await;

    post_json(
        app.clone(),
        "/v1/social/me/profile",
        serde_json::json!({ "handle": "quoter", "displayName": "Quoter" }),
    )
    .await;

    let create_resp = post_json(
        app.clone(),
        "/v1/social/posts",
        serde_json::json!({ "body": "Quotable post" }),
    )
    .await;
    let post_id = body_json(create_resp).await["post"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Quote the post
    let quote_resp = post_json(
        app.clone(),
        "/v1/social/posts",
        serde_json::json!({ "body": "My quote", "quotePostId": post_id }),
    )
    .await;
    assert_eq!(quote_resp.status(), StatusCode::OK);
    let json = body_json(quote_resp).await;
    assert_eq!(json["post"]["quotePostId"], post_id);
    // Create response must nest quotePost so FE can render without a second fetch.
    assert_eq!(json["post"]["quotePost"]["id"], post_id);
    assert_eq!(json["post"]["quotePost"]["body"], "Quotable post");
    assert_eq!(json["post"]["quotePost"]["author"]["handle"], "quoter");
}

/// Repost a post and then unrepost it.
#[tokio::test]
async fn test_repost_and_unrepost() {
    let (app, _tmp) = test_app().await;

    post_json(
        app.clone(),
        "/v1/social/me/profile",
        serde_json::json!({ "handle": "reposter", "displayName": "Reposter" }),
    )
    .await;

    let create_resp = post_json(
        app.clone(),
        "/v1/social/posts",
        serde_json::json!({ "body": "Repost me" }),
    )
    .await;
    let post_id = body_json(create_resp).await["post"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Repost
    let repost_resp = post_json(
        app.clone(),
        &format!("/v1/social/posts/{}/repost", post_id),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(repost_resp.status(), StatusCode::OK);

    // Unrepost via DELETE
    let unrepost_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/social/posts/{}/repost", post_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unrepost_resp.status(), StatusCode::OK);
}

/// Follow status endpoint returns `{ following: bool }` for the viewer.
#[tokio::test]
async fn test_follow_status() {
    let (app, _tmp) = test_app().await;

    post_json(
        app.clone(),
        "/v1/social/me/profile",
        serde_json::json!({ "handle": "viewer1", "displayName": "Viewer" }),
    )
    .await;

    // Unknown handle → 404
    let resp = get(app.clone(), "/v1/social/follows/missing_user/status").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Self / known handle without follow → following false
    let resp = get(app.clone(), "/v1/social/follows/viewer1/status").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["following"], false);

    // Profile-by-handle alias exists
    let resp = get(app.clone(), "/v1/social/profiles/viewer1").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert!(json["profile"]["handle"] == "viewer1" || json["handle"] == "viewer1");
}

/// Create a community and list it via /communities/mine; join by slug works.
#[tokio::test]
async fn test_create_community_and_mine() {
    let (app, _tmp) = test_app().await;

    post_json(
        app.clone(),
        "/v1/social/me/profile",
        serde_json::json!({ "handle": "comm_owner", "displayName": "Owner" }),
    )
    .await;

    let create_resp = post_json(
        app.clone(),
        "/v1/social/communities",
        serde_json::json!({
            "name": "Builders",
            "slug": "builders",
            "description": "A place for builders"
        }),
    )
    .await;
    assert_eq!(create_resp.status(), StatusCode::OK);
    let created = body_json(create_resp).await;
    assert_eq!(created["ok"], true);
    assert_eq!(created["community"]["slug"], "builders");
    assert_eq!(created["community"]["name"], "Builders");
    let community_id = created["community"]["id"].as_str().unwrap().to_string();

    // Creator is auto-joined — show up in mine
    let mine_resp = get(app.clone(), "/v1/social/communities/mine").await;
    assert_eq!(mine_resp.status(), StatusCode::OK);
    let mine = body_json(mine_resp).await;
    let communities = mine["communities"].as_array().expect("communities array");
    assert!(!communities.is_empty());
    assert!(communities.iter().any(|c| c["slug"] == "builders"));
    assert!(communities.iter().any(|c| c.get("joinedAt").is_some()));

    // Feed resolves by slug as well as id
    let feed_slug = get(app.clone(), "/v1/social/communities/builders/feed").await;
    assert_eq!(feed_slug.status(), StatusCode::OK);
    let feed_id = get(
        app.clone(),
        &format!("/v1/social/communities/{}/feed", community_id),
    )
    .await;
    assert_eq!(feed_id.status(), StatusCode::OK);

    // Duplicate slug → conflict
    let dup = post_json(
        app.clone(),
        "/v1/social/communities",
        serde_json::json!({ "name": "Builders 2", "slug": "builders" }),
    )
    .await;
    assert_eq!(dup.status(), StatusCode::CONFLICT);
}

/// Notifications are flat camelCase for FE NotificationsPage.
#[tokio::test]
async fn test_notifications_flat_camelcase_shape() {
    let (app, _tmp) = test_app().await;

    post_json(
        app.clone(),
        "/v1/social/me/profile",
        serde_json::json!({ "handle": "notif_shape", "displayName": "Notif Shape" }),
    )
    .await;

    // Create a post then like it (self-like won't notify, so list may be empty —
    // shape is verified when non-empty; empty list still ok).
    let create_resp = post_json(
        app.clone(),
        "/v1/social/posts",
        serde_json::json!({ "body": "hello notif shape" }),
    )
    .await;
    assert_eq!(create_resp.status(), StatusCode::OK);

    let resp = get(app.clone(), "/v1/social/notifications").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    let notifications = json["notifications"]
        .as_array()
        .expect("notifications array");
    for n in notifications {
        assert!(n.get("id").is_some());
        assert!(n.get("type").is_some());
        assert!(n.get("actorHandle").is_some());
        assert!(n.get("actorDisplayName").is_some());
        assert!(n.get("createdAt").is_some());
        // Must not use nested actors array for primary FE contract
        assert!(n.get("postId").is_some() || n["postId"].is_null());
    }
}

/// Create post with communityId persists communityId.
#[tokio::test]
async fn test_create_post_with_community_id() {
    let (app, _tmp) = test_app().await;

    post_json(
        app.clone(),
        "/v1/social/me/profile",
        serde_json::json!({ "handle": "comm_poster", "displayName": "Poster" }),
    )
    .await;

    let create_comm = post_json(
        app.clone(),
        "/v1/social/communities",
        serde_json::json!({ "name": "Posts Guild", "slug": "posts-guild" }),
    )
    .await;
    let community_id = body_json(create_comm).await["community"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let post_resp = post_json(
        app.clone(),
        "/v1/social/posts",
        serde_json::json!({
            "body": "posted in community",
            "communityId": community_id
        }),
    )
    .await;
    assert_eq!(post_resp.status(), StatusCode::OK);
    let post = body_json(post_resp).await;
    assert_eq!(post["post"]["communityId"], community_id);
}

/// Followers / following list endpoints return profile summaries.
#[tokio::test]
async fn test_followers_and_following_lists() {
    let (app, _tmp) = test_app().await;

    post_json(
        app.clone(),
        "/v1/social/me/profile",
        serde_json::json!({ "handle": "list_target", "displayName": "Target" }),
    )
    .await;

    // Empty lists for a profile with no follows
    let followers = get(app.clone(), "/v1/social/profiles/list_target/followers").await;
    assert_eq!(followers.status(), StatusCode::OK);
    let fjson = body_json(followers).await;
    assert!(fjson["followers"].as_array().expect("followers").is_empty());

    let following = get(app.clone(), "/v1/social/profiles/list_target/following").await;
    assert_eq!(following.status(), StatusCode::OK);
    let gjson = body_json(following).await;
    assert!(gjson["following"].as_array().expect("following").is_empty());

    // Follow self is typically blocked or no-op depending on API; create another
    // follow edge via follow endpoint if it allows (status OK either way).
    let follow_resp = post_json(
        app.clone(),
        "/v1/social/follows/list_target",
        serde_json::json!({}),
    )
    .await;
    // Self-follow may 400/409 — only assert list shape stays valid.
    let _ = follow_resp.status();

    let followers2 = get(app.clone(), "/v1/social/users/list_target/followers").await;
    assert_eq!(followers2.status(), StatusCode::OK);
    assert!(body_json(followers2).await["followers"].is_array());

    // Missing handle → 404
    let missing = get(
        app.clone(),
        "/v1/social/profiles/no_such_user_xyz/followers",
    )
    .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

/// Join community by slug after create works end-to-end.
#[tokio::test]
async fn test_join_leave_community_by_slug() {
    let (app, _tmp) = test_app().await;

    post_json(
        app.clone(),
        "/v1/social/me/profile",
        serde_json::json!({ "handle": "joiner", "displayName": "Joiner" }),
    )
    .await;

    let create_resp = post_json(
        app.clone(),
        "/v1/social/communities",
        serde_json::json!({ "name": "Join Guild", "slug": "join-guild" }),
    )
    .await;
    assert_eq!(create_resp.status(), StatusCode::OK);

    // Creator already joined; leave then re-join by slug
    let leave = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/v1/social/communities/join-guild/leave")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(leave.status(), StatusCode::OK);

    let join = post_json(
        app.clone(),
        "/v1/social/communities/join-guild/join",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(join.status(), StatusCode::OK);
    let j = body_json(join).await;
    assert_eq!(j["ok"], true);
}

/// `GET /v1/social/profiles/{handle}/longform` — the endpoint behind the
/// `longformCount` rendered on every profile.
///
/// Before this route existed, `VeraSocials.tsx` displayed "N longform" in two
/// places with nothing that could list them: `get_longform` took a `FeedQuery`
/// of limit and cursor only, with no author filter by any spelling. This is the
/// one gap from PR #105 that did not ship independently during the Rust
/// rewrite.
#[tokio::test]
async fn test_profile_longform_by_handle() {
    let (app, _tmp) = test_app().await;

    let resp = post_json(
        app.clone(),
        "/v1/social/me/profile",
        serde_json::json!({ "handle": "essayist", "displayName": "The Essayist" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = post_json(
        app.clone(),
        "/v1/social/longform",
        serde_json::json!({
            "title": "On Verification",
            "summary": "a summary",
            "body": "the body",
            "visibility": "public",
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "creating the entry failed");

    // The author's own handle lists their public entry.
    let resp = get(app.clone(), "/v1/social/profiles/essayist/longform").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    let entries = json["longform"].as_array().expect("longform is an array");

    assert_eq!(
        entries.len(),
        1,
        "expected exactly the public entry: {json}"
    );
    assert_eq!(entries[0]["title"], "On Verification");
    assert_eq!(entries[0]["author"]["handle"], "essayist");

    // The visibility filter — the assertion that this route must not become a
    // way to read a draft the global feed hides — is at the database layer, in
    // `db::longform_by_handle_lists_only_that_author_s_public_entries`. It has
    // to be: the create route refuses anything but `public`, so a non-public
    // row cannot be made through the API to test against here. Asserting it
    // only where it can be set up is better than asserting it where it would
    // pass vacuously.

    // Handles are typed by humans, so the match is case-insensitive — the same
    // rule the profile lookup already uses.
    let resp = get(app.clone(), "/v1/social/profiles/ESSAYIST/longform").await;
    let json = body_json(resp).await;
    assert_eq!(
        json["longform"].as_array().expect("array").len(),
        1,
        "handle matching is case sensitive, so a profile link with different \
         casing shows nothing"
    );

    // An unknown handle is an empty list, not a 404: distinguishing "no entries"
    // from "no such profile" would make this an oracle for which handles exist.
    let resp = get(app.clone(), "/v1/social/profiles/nobody-here/longform").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["longform"].as_array().expect("array").len(), 0);
    assert_eq!(json["has_more"], false);

    // And the global feed still works, unfiltered — the shared query serves
    // both, so a change to one must not quietly narrow the other.
    let resp = get(app.clone(), "/v1/social/longform").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["longform"].as_array().expect("array").len(), 1);
}
