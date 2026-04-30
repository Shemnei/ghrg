mod support;

use async_trait::async_trait;
use ghrg_core::policy::{ContextRequest, ContextResolver, Engine, OutcomeVisitor};
use insta::assert_snapshot;
use serde_json::{Map, Value};
use support::{fixture_path, render_diagnostic};

struct EmptyResolver;

#[async_trait(?Send)]
impl ContextResolver for EmptyResolver {
    async fn resolve(
        &self,
        _input: &Value,
        _requests: &[ContextRequest],
    ) -> ghrg_core::Result<Map<String, Value>> {
        Ok(Map::new())
    }
}

#[test]
fn embedded_metadata_yaml_diagnostic_matches_snapshot() {
    let mut engine = Engine::new();
    let error = engine
        .push_file(fixture_path("bad-frontmatter.rego"))
        .unwrap_err();
    assert_snapshot!("embedded_metadata_yaml", render_diagnostic(&error));
}

#[test]
fn sidecar_metadata_yaml_diagnostic_matches_snapshot() {
    let mut engine = Engine::new();
    let error = engine
        .push_file(fixture_path("bad-sidecar.rego"))
        .unwrap_err();
    assert_snapshot!("sidecar_metadata_yaml", render_diagnostic(&error));
}

#[tokio::test(flavor = "current_thread")]
async fn regorus_eval_diagnostic_matches_snapshot() {
    let input = serde_json::json!({"name": "api"});
    let mut engine = Engine::new();
    engine.push_file(fixture_path("bad-eval.rego")).unwrap();
    let engine = engine.finish().unwrap();
    let error = engine
        .run(&input, &EmptyResolver, OutcomeVisitor)
        .await
        .unwrap_err();
    assert_snapshot!("regorus_eval", render_diagnostic(&error));
}
