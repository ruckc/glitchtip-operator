use glitchtip_operator::glitchtip::{ApiError, GlitchTipClient};
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> GlitchTipClient {
    GlitchTipClient::new(reqwest::Client::new(), &server.uri(), "test-token")
}

#[tokio::test]
async fn ping_sends_bearer_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/0/organizations/"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;
    client(&server).ping().await.unwrap();
}

#[tokio::test]
async fn get_organization_maps_404_to_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/0/organizations/missing/"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"detail": "not found"})))
        .mount(&server)
        .await;
    let err = client(&server)
        .get_organization("missing")
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::NotFound));
}

#[tokio::test]
async fn create_organization_posts_name_and_slug() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/0/organizations/"))
        .and(body_json(json!({"name": "Acme Corp", "slug": "acme"})))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": 42,
            "slug": "acme",
            "name": "Acme Corp",
        })))
        .expect(1)
        .mount(&server)
        .await;
    let org = client(&server)
        .create_organization("Acme Corp", Some("acme"))
        .await
        .unwrap();
    assert_eq!(org.slug, "acme");
    // numeric id is normalized to a string
    assert_eq!(org.id.as_deref(), Some("42"));
}

#[tokio::test]
async fn unauthorized_is_distinguished() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/0/organizations/"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"detail": "no"})))
        .mount(&server)
        .await;
    let err = client(&server).ping().await.unwrap_err();
    assert!(matches!(err, ApiError::Unauthorized(_)));
}

#[tokio::test]
async fn create_team_under_organization() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/0/organizations/acme/teams/"))
        .and(body_json(json!({"slug": "backend"})))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({"id": "7", "slug": "backend"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let team = client(&server)
        .create_team("acme", "backend")
        .await
        .unwrap();
    assert_eq!(team.slug, "backend");
    assert_eq!(team.id.as_deref(), Some("7"));
}

#[tokio::test]
async fn create_project_in_team() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/0/teams/acme/backend/projects/"))
        .and(body_json(json!({
            "name": "My App",
            "slug": "my-app",
            "platform": "python-django",
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": 3,
            "slug": "my-app",
            "name": "My App",
            "platform": "python-django",
        })))
        .expect(1)
        .mount(&server)
        .await;
    let project = client(&server)
        .create_project(
            "acme",
            "backend",
            "My App",
            Some("my-app"),
            Some("python-django"),
        )
        .await
        .unwrap();
    assert_eq!(project.slug, "my-app");
}

#[tokio::test]
async fn list_keys_parses_dsn() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/0/projects/acme/my-app/keys/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": "abc123",
            "label": "Default",
            "public": "pubkey",
            "dsn": {
                "public": "https://pubkey@gt.example.com/3",
                "security": "https://gt.example.com/api/3/security/?glitchtip_key=pubkey",
            },
        }])))
        .mount(&server)
        .await;
    let keys = client(&server).list_keys("acme", "my-app").await.unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(
        keys[0].dsn.as_ref().unwrap().public.as_deref(),
        Some("https://pubkey@gt.example.com/3")
    );
}

#[tokio::test]
async fn delete_is_idempotent_on_404() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/0/organizations/gone/"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    client(&server).delete_organization("gone").await.unwrap();
}

#[tokio::test]
async fn tolerates_unknown_fields() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/0/organizations/acme/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "42",
            "slug": "acme",
            "name": "Acme",
            "dateCreated": "2026-01-01T00:00:00Z",
            "isEarlyAdopter": false,
            "features": ["something"],
        })))
        .mount(&server)
        .await;
    let org = client(&server).get_organization("acme").await.unwrap();
    assert_eq!(org.slug, "acme");
}
