use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use cortex_api::state::AppState;

const ROUTE_MANIFEST: &str = include_str!("../route-manifest.csv");
const ROUTER_SOURCE: &str = include_str!("../src/lib.rs");
const CADDYFILE: &str = include_str!("../../../Caddyfile");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Owner {
    Cortex,
    Duplicate,
    Socials,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Contract {
    owner: Owner,
    methods: BTreeSet<String>,
    path: String,
}

fn contracts() -> Vec<Contract> {
    ROUTE_MANIFEST
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut columns = line.splitn(3, ',');
            let owner = match columns.next().expect("owner") {
                "cortex" => Owner::Cortex,
                "duplicate" => Owner::Duplicate,
                "socials" => Owner::Socials,
                unknown => panic!("unknown route owner {unknown}"),
            };
            let methods = columns
                .next()
                .expect("methods")
                .split('|')
                .map(str::to_string)
                .collect();
            let path = columns.next().expect("path").to_string();
            Contract {
                owner,
                methods,
                path,
            }
        })
        .collect()
}

fn function_slice<'a>(source: &'a str, name: &str, next: Option<&str>) -> &'a str {
    let start_marker = format!("pub fn {name}");
    let start = source.find(&start_marker).expect("router function exists");
    let end = next
        .map(|next_name| {
            source[start + start_marker.len()..]
                .find(&format!("pub fn {next_name}"))
                .map(|offset| start + start_marker.len() + offset)
                .expect("next router function exists")
        })
        .unwrap_or(source.len());
    &source[start..end]
}

fn routes_in(source: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut routes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let bytes = source.as_bytes();
    let mut cursor = 0;

    while let Some(relative_start) = source[cursor..].find(".route(") {
        let start = cursor + relative_start;
        let mut index = start + ".route(".len();
        let mut depth = 1_u32;
        let mut in_string = false;
        let mut escaped = false;

        while index < bytes.len() && depth > 0 {
            let byte = bytes[index];
            if in_string {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    in_string = false;
                }
            } else if byte == b'"' {
                in_string = true;
            } else if byte == b'(' {
                depth += 1;
            } else if byte == b')' {
                depth -= 1;
            }
            index += 1;
        }

        assert_eq!(depth, 0, "unbalanced .route call");
        let call = &source[start..index];
        let quote_start = call.find('"').expect("route path starts with a quote") + 1;
        let quote_end = call[quote_start..]
            .find('"')
            .map(|offset| quote_start + offset)
            .expect("route path ends with a quote");
        let path = call[quote_start..quote_end].to_string();
        let methods = routes.entry(path).or_default();
        for method in ["DELETE", "GET", "PATCH", "POST", "PUT"] {
            if call.contains(&format!("{}(", method.to_ascii_lowercase())) {
                methods.insert(method.to_string());
            }
        }
        assert!(!methods.is_empty(), "route has no recognized HTTP method");
        cursor = index;
    }

    routes
}

fn expected_routes(owners: &[Owner]) -> BTreeMap<String, BTreeSet<String>> {
    contracts()
        .into_iter()
        .filter(|contract| owners.contains(&contract.owner))
        .map(|contract| (contract.path, contract.methods))
        .collect()
}

fn caddy_site(label: &str) -> &str {
    let start = CADDYFILE.find(label).expect("Caddy site exists");
    let opening = CADDYFILE[start..]
        .find('{')
        .map(|offset| start + offset)
        .expect("Caddy site opens");
    let mut depth = 0_i32;
    for (offset, byte) in CADDYFILE.as_bytes()[opening..].iter().enumerate() {
        if *byte == b'{' {
            depth += 1;
        } else if *byte == b'}' {
            depth -= 1;
            if depth == 0 {
                return &CADDYFILE[start..=opening + offset];
            }
        }
    }
    panic!("Caddy site block is unbalanced")
}

fn concrete_path(template: &str) -> String {
    template
        .split('/')
        .map(|segment| {
            if segment.starts_with('{') && segment.ends_with('}') {
                "boundary-value"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

async fn send(app: axum::Router, method: &str, path: &str) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method(Method::from_str(method).expect("valid method"))
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .expect("request"),
    )
    .await
    .expect("response")
}

async fn response_json(response: axum::response::Response) -> serde_json::Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).expect("JSON response")
}

#[test]
fn literal_manifest_exactly_matches_both_router_definitions() {
    let manifest = contracts();
    assert_eq!(manifest.len(), 205);
    assert_eq!(
        manifest
            .iter()
            .map(|contract| contract.path.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        205,
        "every path template must have exactly one owner"
    );
    assert_eq!(
        manifest
            .iter()
            .filter(|contract| contract.owner == Owner::Cortex)
            .count(),
        92
    );
    assert_eq!(
        manifest
            .iter()
            .filter(|contract| contract.owner == Owner::Socials)
            .count(),
        98
    );
    assert_eq!(
        manifest
            .iter()
            .filter(|contract| contract.owner == Owner::Duplicate)
            .count(),
        15
    );

    let cortex_source = function_slice(
        ROUTER_SOURCE,
        "build_cortex_router",
        Some("build_heyvera_router"),
    );
    let heyvera_source = function_slice(ROUTER_SOURCE, "build_heyvera_router", None);

    assert_eq!(
        routes_in(cortex_source),
        expected_routes(&[Owner::Cortex, Owner::Duplicate])
    );
    assert_eq!(
        routes_in(heyvera_source),
        expected_routes(&[Owner::Socials, Owner::Duplicate])
    );
}

#[test]
fn caddy_has_explicit_product_matchers_and_deny_fallbacks() {
    let apex = caddy_site("\nheyvera.org, www.heyvera.org {");
    let api = caddy_site("\napi.heyvera.org {");

    for site in [apex, api] {
        let site = site.replace("\r\n", "\n");
        assert!(site.contains("@heyvera_api path /api/health"));
        assert!(site.contains("handle /api/* {\n\t\trespond \"Not found\" 404"));
        assert!(site.contains("handle /v1/* {\n\t\trespond \"Not found\" 404"));
        assert!(!site.contains("handle /api/* {\n\t\treverse_proxy"));
        assert!(!site.contains("localhost:3402"));
    }

    assert!(api.contains("@cortex_api path /api/chat"));
    assert!(api.contains("/api/billing/referral/validate"));
    assert!(apex.contains("/api/clerk/webhooks"));
}

#[tokio::test]
async fn foreign_routes_are_not_reachable_and_duplicates_are_product_specific() {
    std::env::set_var("CORTEX_AUTH_DISABLED", "1");
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(temp.path().join(".cortex")).unwrap();
    std::env::set_var(
        "CORTEX_STATIC_DIR",
        temp.path().join("missing-cortex-static"),
    );
    std::env::set_var(
        "HEYVERA_STATIC_DIR",
        temp.path().join("missing-heyvera-static"),
    );
    let state = AppState::new(
        temp.path().join(".cortex/ledger.jsonl"),
        temp.path().to_path_buf(),
        None,
    )
    .await;
    let cortex = cortex_api::build_cortex_router(state.clone());
    let heyvera = cortex_api::build_heyvera_router(state);

    for contract in contracts() {
        let foreign = match contract.owner {
            Owner::Cortex => Some(heyvera.clone()),
            Owner::Socials => Some(cortex.clone()),
            Owner::Duplicate => None,
        };
        if let Some(app) = foreign {
            let path = concrete_path(&contract.path);
            for method in &contract.methods {
                let response = send(app.clone(), method.as_str(), &path).await;
                assert_eq!(
                    response.status(),
                    StatusCode::NOT_FOUND,
                    "{method} {} leaked into the other product",
                    contract.path
                );
            }
        }
    }

    let cortex_auth = send(cortex.clone(), "GET", "/api/auth/status").await;
    assert_ne!(cortex_auth.status(), StatusCode::NOT_FOUND);
    let heyvera_auth = send(heyvera.clone(), "GET", "/api/auth/status").await;
    assert_eq!(heyvera_auth.status(), StatusCode::NOT_FOUND);

    let cortex_health = send(cortex.clone(), "GET", "/v1/health").await;
    assert_eq!(response_json(cortex_health).await["service"], "cortex");
    let heyvera_health = send(heyvera.clone(), "GET", "/v1/health").await;
    assert_eq!(
        response_json(heyvera_health).await["service"],
        "heyvera-social"
    );

    let cortex_usage = send(cortex, "GET", "/api/billing/usage").await;
    assert_eq!(cortex_usage.status(), StatusCode::OK);
    let heyvera_usage = send(heyvera, "GET", "/api/billing/usage").await;
    assert_eq!(heyvera_usage.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response_json(heyvera_usage).await["code"],
        "SOCIALS_BILLING_UNAVAILABLE"
    );
}
