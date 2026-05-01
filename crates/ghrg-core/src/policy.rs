use async_trait::async_trait;
use miette::{NamedSource, SourceSpan};
use regorus::CompiledPolicy;
use regorus::Rc;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub use crate::contexts::{ContextSpec, ContextValue, ContextValueRef, DynamicContextData};

use crate::error::{GhrgError, Result};

#[derive(Debug, Clone, Copy)]
pub struct Building;
#[derive(Debug, Clone, Copy)]
pub struct Finished;

#[derive(Debug, Clone)]
pub struct Policy {
    pub path: PathBuf,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct ContextRequest {
    pub key: String,
    pub spec: ContextSpec,
}

impl ContextRequest {
    fn resolve_dynamic(&self, runtime: &DynamicContextData<'_>) -> Result<Self> {
        Ok(Self {
            key: self.key.clone(),
            spec: self.spec.resolve_dynamic(runtime)?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Engine<State = Building> {
    policies: Vec<CompiledPolicyUnit>,
    context_specs: Vec<ContextSpec>,
    _state: std::marker::PhantomData<State>,
}

#[derive(Debug, Clone)]
pub struct RunStep {
    pub policy: PathBuf,
    pub package: String,
    pub metadata: Option<LoadedPolicyMetadata>,
    pub keep: bool,
    pub output: PolicyResult,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone)]
pub struct StepControl {
    pub keep: bool,
    pub next_input: Value,
    pub output: PolicyResult,
    pub stop: bool,
}

#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub keep: bool,
    pub result: PolicyResult,
    pub dropped_by: Option<PathBuf>,
    pub steps: Vec<RunStep>,
}

#[async_trait(?Send)]
pub trait ContextResolver {
    async fn resolve(
        &self,
        input: &Value,
        requests: &[ContextRequest],
    ) -> Result<Map<String, Value>>;
}

#[async_trait(?Send)]
pub trait RunVisitor: Sized {
    type Output;

    async fn begin(&mut self, _engine: &Engine<Finished>, _input: &Value) -> Result<()> {
        Ok(())
    }

    async fn step(&mut self, _step: &RunStep, _control: &mut StepControl) -> Result<()> {
        Ok(())
    }

    async fn finish(self, outcome: RunOutcome) -> Result<Self::Output>;
}

pub trait RunLayer<V> {
    type Visitor;

    fn layer(self, inner: V) -> Self::Visitor;
}

#[async_trait(?Send)]
pub trait RunTap {
    async fn begin(&mut self, _engine: &Engine<Finished>, _input: &Value) -> Result<()> {
        Ok(())
    }

    async fn step(&mut self, _step: &RunStep, _control: &mut StepControl) -> Result<()> {
        Ok(())
    }

    async fn finish(&mut self, _outcome: &mut RunOutcome) -> Result<()> {
        Ok(())
    }
}

pub struct LayeredVisitor<L, V> {
    layer: L,
    inner: V,
}

pub struct OutcomeVisitor;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub contexts: Vec<ContextSpec>,
}

impl Policy {
    pub fn new(path: impl Into<PathBuf>, source: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            source: source.into(),
        }
    }
}

impl Engine<Building> {
    pub fn new() -> Self {
        Self {
            policies: Vec::new(),
            context_specs: Vec::new(),
            _state: std::marker::PhantomData,
        }
    }

    pub fn push_file(&mut self, path: impl Into<PathBuf>) -> Result<&mut Self> {
        let path = path.into();
        let source = fs::read_to_string(&path).map_err(|source| GhrgError::PolicyRead {
            path: path.display().to_string(),
            source,
        })?;
        self.push_policy(Policy::new(path, source))
    }

    pub fn push_policy(&mut self, policy: Policy) -> Result<&mut Self> {
        let compiled = compile_policy_unit(policy)?;
        self.policies.push(compiled);
        Ok(self)
    }

    pub fn finish(self) -> Result<Engine<Finished>> {
        let context_specs = collect_context_specs(&self.policies);
        Ok(Engine {
            policies: self.policies,
            context_specs,
            _state: std::marker::PhantomData,
        })
    }
}

impl Default for Engine<Building> {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine<Finished> {
    pub fn policies(&self) -> &[CompiledPolicyUnit] {
        &self.policies
    }

    pub fn context_specs(&self) -> &[ContextSpec] {
        &self.context_specs
    }

    pub async fn run<R, V>(&self, input: &Value, resolver: &R, mut visitor: V) -> Result<V::Output>
    where
        R: ContextResolver,
        V: RunVisitor,
    {
        visitor.begin(self, input).await?;

        let mut current = input.clone();
        let mut keep = true;
        let mut dropped_by = None;
        let mut steps = Vec::new();
        let mut context_cache = Vec::<(ContextSpec, Value)>::new();
        let mut meta_last = None::<Value>;
        let mut meta_policies = Map::<String, Value>::new();

        for policy in &self.policies {
            let runtime = DynamicContextData::new(&current, meta_last.as_ref(), &meta_policies);
            let dynamic_requests = policy
                .requests
                .iter()
                .map(|request| request.resolve_dynamic(&runtime))
                .collect::<Result<Vec<_>>>()?;

            let contexts =
                resolve_policy_contexts(resolver, &current, &dynamic_requests, &mut context_cache)
                    .await?;

            let step = evaluate_compiled_policy(policy, &attach_contexts(&current, contexts))?;
            let mut control = StepControl {
                keep: step.keep,
                next_input: step.output.object.json_value(),
                output: step.output.clone(),
                stop: !step.keep,
            };
            visitor.step(&step, &mut control).await?;

            let final_step = RunStep {
                policy: step.policy.clone(),
                package: step.package.clone(),
                metadata: step.metadata.clone(),
                keep: control.keep,
                output: control.output.clone(),
                elapsed_ms: step.elapsed_ms,
            };

            if control.keep {
                current = control.next_input.clone();
            } else {
                keep = false;
                dropped_by = Some(final_step.policy.clone());
            }

            if let Some(meta) = final_step.output.meta.clone() {
                meta_last = Some(meta.clone());
                for key in policy_meta_keys(policy) {
                    meta_policies.insert(key, meta.clone());
                }
            }

            steps.push(final_step);

            if control.stop || !keep {
                break;
            }
        }

        let result = steps
            .last()
            .map(|step| step.output.clone())
            .unwrap_or_else(|| {
                PolicyResult::from_serializable(&current).expect("run input should serialize")
            });
        visitor
            .finish(RunOutcome {
                keep,
                result,
                dropped_by,
                steps,
            })
            .await
    }
}

impl<L, V> RunLayer<V> for L
where
    L: RunTap,
{
    type Visitor = LayeredVisitor<L, V>;

    fn layer(self, inner: V) -> Self::Visitor {
        LayeredVisitor { layer: self, inner }
    }
}

#[async_trait(?Send)]
impl<L, V> RunVisitor for LayeredVisitor<L, V>
where
    L: RunTap,
    V: RunVisitor,
{
    type Output = V::Output;

    async fn begin(&mut self, engine: &Engine<Finished>, input: &Value) -> Result<()> {
        self.layer.begin(engine, input).await?;
        self.inner.begin(engine, input).await
    }

    async fn step(&mut self, step: &RunStep, control: &mut StepControl) -> Result<()> {
        self.layer.step(step, control).await?;
        self.inner.step(step, control).await
    }

    async fn finish(mut self, mut outcome: RunOutcome) -> Result<Self::Output> {
        self.layer.finish(&mut outcome).await?;
        self.inner.finish(outcome).await
    }
}

#[async_trait(?Send)]
impl RunVisitor for OutcomeVisitor {
    type Output = RunOutcome;

    async fn finish(self, outcome: RunOutcome) -> Result<Self::Output> {
        Ok(outcome)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutputObject {
    pub fields: Vec<OutputField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputField {
    pub name: String,
    pub value: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyResult {
    pub object: OutputObject,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
struct PolicySource {
    pub path: PathBuf,
    pub package: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub enum MetadataSourceKind {
    Embedded,
    Sidecar,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoadedPolicyMetadata {
    pub source: MetadataSourceKind,
    pub metadata: PolicyMetadata,
}

#[derive(Debug, Clone)]
pub struct CompiledPolicyUnit {
    pub path: PathBuf,
    pub package: String,
    pub metadata: Option<LoadedPolicyMetadata>,
    requests: Vec<ContextRequest>,
    source: PolicySource,
    compiled_allow: Option<CompiledPolicy>,
    compiled_output: Option<CompiledPolicy>,
    compiled_meta: Option<CompiledPolicy>,
}

struct MetadataDocument {
    source: MetadataSourceKind,
    path: String,
    src: NamedSource<String>,
    yaml: String,
    base_offset: usize,
}

impl PolicyResult {
    pub fn new(object: OutputObject) -> Self {
        Self { object, meta: None }
    }

    pub fn from_serializable<T: Serialize>(value: &T) -> Result<Self> {
        Ok(Self::new(OutputObject::from_serializable(value)?))
    }

    pub fn with_meta(mut self, meta: impl Into<Value>) -> Self {
        self.meta = Some(meta.into());
        self
    }
}

impl OutputObject {
    pub fn new(fields: Vec<OutputField>) -> Self {
        Self { fields }
    }

    pub fn field(&self, name: &str) -> Option<&Value> {
        self.fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| &field.value)
    }

    pub fn field_names(&self) -> Vec<&str> {
        self.fields
            .iter()
            .map(|field| field.name.as_str())
            .collect()
    }

    pub fn json_value(&self) -> Value {
        let mut map = Map::new();
        for field in &self.fields {
            map.insert(field.name.clone(), field.value.clone());
        }
        Value::Object(map)
    }

    pub fn from_serializable<T: Serialize>(value: &T) -> Result<Self> {
        match serde_json::to_value(value)? {
            Value::Object(map) => Ok(Self::from_json_object(map)),
            _ => Err(GhrgError::OutputObjectRequiresJsonObject),
        }
    }

    pub fn from_json_object(map: Map<String, Value>) -> Self {
        let fields = map
            .into_iter()
            .map(|(name, value)| OutputField::new(name, value))
            .collect();
        Self::new(fields)
    }
}

impl OutputField {
    pub fn new(name: impl Into<String>, value: impl Into<Value>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

fn compile_policy_unit(policy: Policy) -> Result<CompiledPolicyUnit> {
    let started_at = Instant::now();
    let source = PolicySource {
        path: policy.path,
        package: parse_package(&policy.source),
        source: policy.source,
    };
    let path = source.path.clone();
    let _package = source
        .package
        .clone()
        .ok_or(GhrgError::MissingPolicyPackage {
            path: path.display().to_string(),
            src: source.named_source(),
            span: source
                .package_span()
                .unwrap_or_else(|| source.primary_span()),
        })?;
    let metadata = load_policy_metadata(&source)?;

    let mut engine = regorus::Engine::new();
    let package = engine
        .add_policy(path.display().to_string(), source.source.clone())
        .map_err(|error| source.evaluation_error("policy load", error.to_string()))?;
    let compiled_allow = compile_entrypoint_if_present(&mut engine, &source, &package, "allow")?;
    let compiled_output = compile_entrypoint_if_present(&mut engine, &source, &package, "output")?;
    let compiled_meta = compile_entrypoint_if_present(&mut engine, &source, &package, "meta")?;
    let requests = context_requests_for_metadata(&metadata);

    tracing::trace!(
        policy = %path.display(),
        package = package.as_str(),
        elapsed_ms = started_at.elapsed().as_millis(),
        "compiled policy"
    );

    Ok(CompiledPolicyUnit {
        path,
        package,
        metadata,
        requests,
        source,
        compiled_allow,
        compiled_output,
        compiled_meta,
    })
}

fn evaluate_compiled_policy(policy: &CompiledPolicyUnit, input: &Value) -> Result<RunStep> {
    let started_at = Instant::now();

    let allow = eval_compiled_rule(policy, policy.compiled_allow.as_ref(), "allow", input)?
        .and_then(|value| value.as_bool().ok().copied())
        .unwrap_or(false);

    let output_value =
        eval_compiled_rule(policy, policy.compiled_output.as_ref(), "output", input)?;
    let meta_value = eval_compiled_rule(policy, policy.compiled_meta.as_ref(), "meta", input)?;

    let base_object = output_value
        .as_ref()
        .map(regorus_to_json)
        .transpose()?
        .unwrap_or_else(|| input.clone());

    let mut result = PolicyResult::from_serializable(&base_object)?;

    if let Some(meta) = meta_value.as_ref().map(regorus_to_json).transpose()? {
        result = result.with_meta(meta);
    }

    Ok(RunStep {
        policy: policy.path.clone(),
        keep: allow,
        output: result,
        package: policy.package.clone(),
        metadata: policy.metadata.clone(),
        elapsed_ms: started_at.elapsed().as_millis(),
    })
}

fn compile_entrypoint_if_present(
    engine: &mut regorus::Engine,
    source: &PolicySource,
    package: &str,
    rule: &str,
) -> Result<Option<CompiledPolicy>> {
    if !source.has_rule(rule) {
        return Ok(None);
    }

    let entrypoint: Rc<str> = query_rule_name(package, rule).into();
    engine
        .compile_with_entrypoint(&entrypoint)
        .map(Some)
        .map_err(|error| source.evaluation_error(entrypoint.to_string(), error.to_string()))
}

fn eval_compiled_rule(
    policy: &CompiledPolicyUnit,
    compiled: Option<&CompiledPolicy>,
    rule: &str,
    input: &Value,
) -> Result<Option<regorus::Value>> {
    let Some(compiled) = compiled else {
        return Ok(None);
    };

    let value = compiled
        .eval_with_input(regorus::Value::from(input.clone()))
        .map_err(|error| {
            policy
                .source
                .evaluation_error(query_rule_name(&policy.package, rule), error.to_string())
        })?;

    if value == regorus::Value::Undefined {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

fn context_requests_for_metadata(metadata: &Option<LoadedPolicyMetadata>) -> Vec<ContextRequest> {
    metadata
        .as_ref()
        .map(|loaded| {
            loaded
                .metadata
                .contexts
                .iter()
                .map(|spec| ContextRequest {
                    key: spec.input_key().to_string(),
                    spec: spec.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn policy_meta_keys(policy: &CompiledPolicyUnit) -> Vec<String> {
    let mut keys = BTreeSet::new();
    keys.insert(policy.path.display().to_string());

    if let Some(name) = policy
        .metadata
        .as_ref()
        .and_then(|loaded| loaded.metadata.name.as_deref())
        .filter(|value| !value.is_empty())
    {
        keys.insert(name.to_string());
    }

    keys.into_iter().collect()
}

fn collect_context_specs(policies: &[CompiledPolicyUnit]) -> Vec<ContextSpec> {
    let mut context_specs = Vec::new();

    for policy in policies {
        for request in &policy.requests {
            if context_specs
                .iter()
                .any(|existing| existing == &request.spec)
            {
                continue;
            }

            context_specs.push(request.spec.clone());
        }
    }

    context_specs
}

async fn resolve_policy_contexts<R>(
    resolver: &R,
    input: &Value,
    requests: &[ContextRequest],
    cache: &mut Vec<(ContextSpec, Value)>,
) -> Result<Map<String, Value>>
where
    R: ContextResolver,
{
    let missing = missing_context_requests(requests, cache);
    if !missing.is_empty() {
        let resolved = resolver.resolve(input, &missing).await?;
        for request in &missing {
            if let Some(value) = resolved.get(&request.key) {
                cache.push((request.spec.clone(), value.clone()));
            }
        }
    }

    let mut contexts = Map::new();
    for request in requests {
        if let Some((_, value)) = cache
            .iter()
            .find(|(spec, _)| spec.same_provider(&request.spec))
        {
            contexts.insert(request.key.clone(), value.clone());
        }
    }

    Ok(contexts)
}

fn missing_context_requests(
    requests: &[ContextRequest],
    cache: &[(ContextSpec, Value)],
) -> Vec<ContextRequest> {
    let mut missing = Vec::new();

    for request in requests {
        if cache
            .iter()
            .any(|(spec, _)| spec.same_provider(&request.spec))
            || missing
                .iter()
                .any(|existing: &ContextRequest| existing.spec.same_provider(&request.spec))
        {
            continue;
        }

        missing.push(request.clone());
    }

    missing
}

fn attach_contexts(input: &Value, contexts: Map<String, Value>) -> Value {
    match input.clone() {
        Value::Object(mut map) => {
            map.insert("contexts".to_string(), Value::Object(contexts));
            Value::Object(map)
        }
        value => serde_json::json!({
            "input": value,
            "contexts": contexts,
        }),
    }
}

impl PolicySource {
    fn named_source(&self) -> NamedSource<String> {
        NamedSource::new(self.path.display().to_string(), self.source.clone())
    }

    fn has_rule(&self, rule: &str) -> bool {
        self.rule_span(rule).is_some()
    }

    fn primary_span(&self) -> SourceSpan {
        (
            0,
            self.source.lines().next().map(str::len).unwrap_or(1).max(1),
        )
            .into()
    }

    fn package_span(&self) -> Option<SourceSpan> {
        self.find_trimmed_line_span(|trimmed| trimmed.starts_with("package "))
    }

    fn evaluation_error(&self, rule: impl Into<String>, message: String) -> GhrgError {
        let rule = rule.into();
        let span = parse_regorus_error_span(&self.source, &message)
            .or_else(|| self.rule_span(&rule))
            .or_else(|| self.package_span())
            .unwrap_or_else(|| self.primary_span());

        GhrgError::PolicyEvaluation {
            path: self.path.display().to_string(),
            rule,
            message,
            src: self.named_source(),
            span,
        }
    }

    fn rule_span(&self, rule: &str) -> Option<SourceSpan> {
        let short_rule = rule.rsplit('.').next().unwrap_or(rule);
        self.find_trimmed_line_span(|trimmed| {
            trimmed.starts_with(&format!("{short_rule} "))
                || trimmed.starts_with(&format!("{short_rule}:="))
                || trimmed.starts_with(&format!("{short_rule} :="))
                || trimmed.starts_with(&format!("default {short_rule} "))
        })
    }

    fn find_trimmed_line_span(&self, predicate: impl Fn(&str) -> bool) -> Option<SourceSpan> {
        for (offset, line) in source_line_segments(&self.source) {
            let trimmed = line.trim();
            if predicate(trimmed) {
                return Some((offset, line.len().max(1)).into());
            }
        }
        None
    }
}

fn source_line_segments(source: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut offset = 0usize;

    source.split_inclusive('\n').map(move |segment| {
        let current_offset = offset;
        offset += segment.len();

        let line = segment.strip_suffix('\n').unwrap_or(segment);
        let line = line.strip_suffix('\r').unwrap_or(line);
        (current_offset, line)
    })
}

fn load_policy_metadata(source: &PolicySource) -> Result<Option<LoadedPolicyMetadata>> {
    let Some(document) = load_metadata_document(source)? else {
        return Ok(None);
    };

    Ok(Some(LoadedPolicyMetadata {
        source: document.source.clone(),
        metadata: deserialize_metadata_document(&document)?,
    }))
}

#[cfg(test)]
fn parse_embedded_metadata(source: &PolicySource) -> Result<Option<PolicyMetadata>> {
    load_metadata_document(source)?
        .filter(|document| matches!(document.source, MetadataSourceKind::Embedded))
        .map(|document| deserialize_metadata_document(&document))
        .transpose()
}

fn parse_package(source: &str) -> Option<String> {
    source.lines().find_map(|line| {
        line.trim()
            .strip_prefix("package ")
            .map(|value| value.trim().to_string())
    })
}

fn load_metadata_document(source: &PolicySource) -> Result<Option<MetadataDocument>> {
    let embedded = parse_embedded_metadata_document(source)?;
    let sidecar = parse_sidecar_metadata_document(&source.path)?;

    match (embedded, sidecar) {
        (Some(_), Some(_)) => Err(GhrgError::PolicyMetadataConflict {
            path: source.path.display().to_string(),
        }),
        (Some(document), None) | (None, Some(document)) => Ok(Some(document)),
        (None, None) => Ok(None),
    }
}

fn deserialize_metadata_document(document: &MetadataDocument) -> Result<PolicyMetadata> {
    let raw = serde_yaml::from_str::<serde_yaml::Value>(&document.yaml)
        .map_err(|error| metadata_yaml_error(document, error))?;
    let metadata = serde_yaml::from_value::<PolicyMetadata>(raw.clone())
        .map_err(|error| metadata_yaml_error(document, error))?;
    validate_metadata_contexts(document, &raw, &metadata.contexts)?;
    Ok(metadata)
}

fn validate_metadata_contexts(
    document: &MetadataDocument,
    raw: &serde_yaml::Value,
    contexts: &[ContextSpec],
) -> Result<()> {
    let Some(mapping) = raw.as_mapping() else {
        return Ok(());
    };

    if let Some(contexts_value) = mapping.get("contexts")
        && !contexts_value.is_sequence()
    {
        return Err(metadata_field_error(
            document,
            "contexts",
            "`contexts` must be a list",
        ));
    }

    validate_context_specs(document, contexts)
}

fn validate_context_specs(document: &MetadataDocument, contexts: &[ContextSpec]) -> Result<()> {
    let mut seen_keys = std::collections::BTreeSet::new();

    for context in contexts {
        let input_key = context.input_key();
        if !seen_keys.insert(input_key.to_string()) {
            return Err(metadata_field_error(
                document,
                "contexts",
                &format!("duplicate context input key `{input_key}`"),
            ));
        }

        if context.name.as_deref().is_some_and(str::is_empty) {
            return Err(metadata_field_error(
                document,
                "contexts",
                "`context.name` must be a non-empty string",
            ));
        }

        validate_context_spec(document, context)?;
    }

    Ok(())
}

fn validate_context_spec(document: &MetadataDocument, context: &ContextSpec) -> Result<()> {
    context
        .validate()
        .map_err(|message| metadata_field_error(document, "contexts", &message))
}

fn metadata_field_error(document: &MetadataDocument, field: &str, message: &str) -> GhrgError {
    GhrgError::PolicyMetadataValidation {
        path: document.path.clone(),
        message: message.to_string(),
        src: document.src.clone(),
        span: metadata_field_span(document, field),
    }
}

fn parse_embedded_metadata_document(source: &PolicySource) -> Result<Option<MetadataDocument>> {
    let mut yaml_lines = Vec::new();
    let mut saw_package = false;
    let mut in_block = false;
    let mut saw_open = false;
    let mut open_span = None;
    let mut yaml_start = None;
    let mut yaml_end = None;

    for (offset, line) in source_line_segments(&source.source) {
        let trimmed = line.trim();

        if !saw_open && trimmed.starts_with("package ") {
            saw_package = true;
        }

        if trimmed == "# ```ghrg" {
            if saw_package {
                return Err(GhrgError::EmbeddedMetadataAfterPackage {
                    path: source.path.display().to_string(),
                    src: source.named_source(),
                    span: (offset, line.len().max(1)).into(),
                });
            }
            saw_open = true;
            in_block = true;
            open_span = Some((offset, line.len().max(1)).into());
            continue;
        }

        if in_block {
            if trimmed == "# ```" {
                return Ok(Some(MetadataDocument {
                    source: MetadataSourceKind::Embedded,
                    path: source.path.display().to_string(),
                    src: source.named_source(),
                    yaml: yaml_lines.join("\n"),
                    base_offset: yaml_start.unwrap_or(offset),
                }));
            }

            if !trimmed.starts_with('#') {
                return Err(GhrgError::MalformedEmbeddedMetadataFence {
                    path: source.path.display().to_string(),
                    src: source.named_source(),
                    span: open_span.unwrap_or_else(|| source.primary_span()),
                });
            }

            let content = trimmed.strip_prefix('#').unwrap_or_default();
            let content = content.strip_prefix(' ').unwrap_or(content);
            yaml_start.get_or_insert(offset);
            yaml_end = Some(offset + line.len());
            yaml_lines.push(content.to_string());
        }
    }

    if saw_open {
        return Err(GhrgError::MalformedEmbeddedMetadataFence {
            path: source.path.display().to_string(),
            src: source.named_source(),
            span: if let (Some(start), Some(end)) = (yaml_start, yaml_end) {
                (start, end.saturating_sub(start).max(1)).into()
            } else {
                open_span.unwrap_or_else(|| source.primary_span())
            },
        });
    }

    Ok(None)
}

fn parse_sidecar_metadata_document(path: &Path) -> Result<Option<MetadataDocument>> {
    let Some(sidecar_path) = sidecar_path(path) else {
        return Ok(None);
    };

    if !sidecar_path.exists() {
        return Ok(None);
    }

    let source = fs::read_to_string(&sidecar_path).map_err(|source| GhrgError::PolicyRead {
        path: sidecar_path.display().to_string(),
        source,
    })?;

    Ok(Some(MetadataDocument {
        source: MetadataSourceKind::Sidecar,
        path: sidecar_path.display().to_string(),
        src: NamedSource::new(sidecar_path.display().to_string(), source.clone()),
        yaml: source,
        base_offset: 0,
    }))
}

fn sidecar_path(path: &Path) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_string_lossy();
    Some(path.with_file_name(format!("{stem}.ghrg.yaml")))
}

fn regorus_to_json(value: &regorus::Value) -> Result<Value> {
    let json = value
        .to_json_str()
        .map_err(|error| GhrgError::PolicyEvaluation {
            path: "<in-memory>".to_string(),
            rule: "serialization".to_string(),
            message: error.to_string(),
            src: NamedSource::new("<in-memory>", String::new()),
            span: (0, 1).into(),
        })?;
    Ok(serde_json::from_str(&json)?)
}

fn parse_regorus_error_span(source: &str, message: &str) -> Option<SourceSpan> {
    let marker = "--> ";
    let line = message
        .lines()
        .find(|line| line.trim_start().starts_with(marker))?;
    let location = line.trim_start().strip_prefix(marker)?;
    let mut parts = location.rsplitn(3, ':');
    let column = parts.next()?.parse::<usize>().ok()?;
    let line = parts.next()?.parse::<usize>().ok()?;
    let _path = parts.next()?;

    Some((line_column_to_offset(source, line, column), 1).into())
}

fn yaml_error_span(
    source: &str,
    base_offset: usize,
    location: Option<(usize, usize)>,
) -> SourceSpan {
    if let Some((line, column)) = location {
        let offset = base_offset + line_column_to_offset(&source[base_offset..], line, column);
        (offset, 1).into()
    } else {
        (base_offset, 1).into()
    }
}

fn metadata_yaml_error(document: &MetadataDocument, error: serde_yaml::Error) -> GhrgError {
    match document.source {
        MetadataSourceKind::Embedded => GhrgError::EmbeddedMetadataYaml {
            path: document.path.clone(),
            src: document.src.clone(),
            span: yaml_error_span(
                document.src.inner(),
                document.base_offset,
                error
                    .location()
                    .map(|location| (location.line(), location.column())),
            ),
            source: error,
        },
        MetadataSourceKind::Sidecar => GhrgError::SidecarMetadataYaml {
            path: document.path.clone(),
            src: document.src.clone(),
            span: yaml_error_span(
                document.src.inner(),
                document.base_offset,
                error
                    .location()
                    .map(|location| (location.line(), location.column())),
            ),
            source: error,
        },
    }
}

fn metadata_field_span(document: &MetadataDocument, field: &str) -> SourceSpan {
    let pattern = format!("{field}:");

    for (index, line) in document.yaml.lines().enumerate() {
        if let Some(column) = line.find(&pattern) {
            let offset =
                document.base_offset + line_column_to_offset(&document.yaml, index + 1, column + 1);
            return (offset, pattern.len()).into();
        }
    }

    (document.base_offset, field.len().max(1)).into()
}

fn line_column_to_offset(source: &str, line: usize, column: usize) -> usize {
    let mut offset = 0usize;

    for (current_line, segment) in (1usize..).zip(source.split_inclusive('\n')) {
        if current_line == line {
            return offset
                + column
                    .saturating_sub(1)
                    .min(segment.len().saturating_sub(1));
        }
        offset += segment.len();
    }

    0
}

fn query_rule_name(package: &str, rule: &str) -> String {
    if package.starts_with("data.") {
        format!("{package}.{rule}")
    } else {
        format!("data.{package}.{rule}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct EmptyResolver;

    #[async_trait(?Send)]
    impl ContextResolver for EmptyResolver {
        async fn resolve(
            &self,
            _input: &Value,
            _requests: &[ContextRequest],
        ) -> Result<Map<String, Value>> {
            Ok(Map::new())
        }
    }

    struct ForceKeepLayer;

    #[async_trait(?Send)]
    impl RunTap for ForceKeepLayer {
        async fn step(&mut self, _step: &RunStep, control: &mut StepControl) -> Result<()> {
            control.keep = true;
            control.stop = false;
            Ok(())
        }
    }

    #[derive(Clone, Default)]
    struct RecordingResolver {
        calls: Arc<Mutex<Vec<Vec<String>>>>,
    }

    #[async_trait(?Send)]
    impl ContextResolver for RecordingResolver {
        async fn resolve(
            &self,
            _input: &Value,
            requests: &[ContextRequest],
        ) -> Result<Map<String, Value>> {
            self.calls
                .lock()
                .unwrap()
                .push(requests.iter().map(context_descriptor).collect());

            Ok(Map::from_iter(requests.iter().map(|request| {
                (
                    request.key.clone(),
                    Value::String(format!("value:{}", context_descriptor(request))),
                )
            })))
        }
    }

    fn context_descriptor(request: &ContextRequest) -> String {
        let params = crate::contexts::repo::render_context_params(&request.spec)
            .unwrap_or_else(|_| String::new());
        if params.is_empty() {
            request.spec.kind().to_string()
        } else {
            format!("{}({params})", request.spec.kind())
        }
    }

    #[test]
    fn parses_embedded_metadata() {
        let source = PolicySource {
            path: PathBuf::from("test.rego"),
            package: Some("ghrg.repos".to_string()),
            source: "# ```ghrg\n# name: sample\n# contexts:\n#   - name: recent_commits\n#     type: commits\n#     params:\n#       limit: 1\n# ```\n\npackage ghrg.repos\n".to_string(),
        };

        let metadata = parse_embedded_metadata(&source).unwrap().unwrap();
        assert_eq!(metadata.name.as_deref(), Some("sample"));
        assert_eq!(metadata.contexts[0].kind(), "commits");
        assert_eq!(metadata.contexts[0].name.as_deref(), Some("recent_commits"));
    }

    #[test]
    fn rejects_duplicate_context_input_keys() {
        let source = PolicySource {
            path: PathBuf::from("test.rego"),
            package: Some("ghrg.repos".to_string()),
            source: "# ```ghrg\n# contexts:\n#   - type: files\n#   - type: languages\n#     name: files\n# ```\n\npackage ghrg.repos\n".to_string(),
        };

        let error = parse_embedded_metadata(&source).unwrap_err();
        assert!(matches!(error, GhrgError::PolicyMetadataValidation { .. }));
        assert!(
            error
                .to_string()
                .contains("duplicate context input key `files`")
        );
    }

    #[test]
    fn rejects_embedded_metadata_after_package() {
        let source = PolicySource {
            path: PathBuf::from("test.rego"),
            package: Some("ghrg.repos".to_string()),
            source: "package ghrg.repos\n# ```ghrg\n# name: sample\n# ```\n".to_string(),
        };

        let error = parse_embedded_metadata(&source).unwrap_err();
        assert!(matches!(
            error,
            GhrgError::EmbeddedMetadataAfterPackage { .. }
        ));
    }

    #[test]
    fn loads_sidecar_metadata() {
        let temp_dir = temp_dir();
        let policy_path = temp_dir.join("sample.rego");
        let sidecar_path = temp_dir.join("sample.ghrg.yaml");
        fs::write(&policy_path, "package ghrg.repos\n").unwrap();
        fs::write(&sidecar_path, "name: sample\n").unwrap();

        let source = PolicySource {
            path: policy_path,
            package: Some("ghrg.repos".to_string()),
            source: "package ghrg.repos\n".to_string(),
        };
        let metadata = load_policy_metadata(&source).unwrap().unwrap();
        assert!(matches!(metadata.source, MetadataSourceKind::Sidecar));
        assert_eq!(metadata.metadata.name.as_deref(), Some("sample"));
    }

    #[test]
    fn rejects_conflicting_metadata_sources() {
        let temp_dir = temp_dir();
        let policy_path = temp_dir.join("sample.rego");
        let sidecar_path = temp_dir.join("sample.ghrg.yaml");
        fs::write(
            &policy_path,
            "# ```ghrg\n# name: embedded\n# ```\n\npackage ghrg.repos\n",
        )
        .unwrap();
        fs::write(&sidecar_path, "name: sidecar\n").unwrap();

        let source = PolicySource {
            path: policy_path,
            package: Some("ghrg.repos".to_string()),
            source: "# ```ghrg\n# name: embedded\n# ```\n\npackage ghrg.repos\n".to_string(),
        };
        let error = load_policy_metadata(&source).unwrap_err();
        assert!(matches!(error, GhrgError::PolicyMetadataConflict { .. }));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn evaluates_policy_test_chain() {
        let temp_dir = temp_dir();
        let filter = temp_dir.join("filter.rego");
        let project = temp_dir.join("project.rego");
        fs::write(
            &filter,
            "package ghrg.repos\n\ndefault allow := false\n\nallow if { input.archived == false }\noutput := input\n",
        )
        .unwrap();
        fs::write(
            &project,
            "package ghrg.repos\n\ndefault allow := false\n\nallow if { input.name != \"\" }\noutput := {\"name\": input.name, \"team\": input.team}\nmeta := {\"selected\": [\"name\", \"team\"]}\n",
        )
        .unwrap();

        let input = serde_json::json!({"name": "api", "team": "platform", "archived": false});
        let mut engine = Engine::new();
        engine.push_file(filter).unwrap();
        engine.push_file(project).unwrap();
        let engine = engine.finish().unwrap();
        let outcome = engine
            .run(&input, &EmptyResolver, OutcomeVisitor)
            .await
            .unwrap();

        assert!(outcome.keep);
        assert_eq!(outcome.steps.len(), 2);
        assert_eq!(
            outcome.result.object.field("name"),
            Some(&Value::String("api".to_string()))
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stops_policy_chain_on_drop() {
        let temp_dir = temp_dir();
        let filter = temp_dir.join("filter.rego");
        let project = temp_dir.join("project.rego");
        fs::write(
            &filter,
            "package ghrg.repos\n\ndefault allow := false\n\nallow if { input.archived == false }\noutput := input\n",
        )
        .unwrap();
        fs::write(
            &project,
            "package ghrg.repos\n\ndefault allow := true\noutput := {\"name\": input.name}\n",
        )
        .unwrap();

        let input = serde_json::json!({"name": "api", "archived": true});
        let mut engine = Engine::new();
        engine.push_file(filter.clone()).unwrap();
        engine.push_file(project).unwrap();
        let engine = engine.finish().unwrap();
        let outcome = engine
            .run(&input, &EmptyResolver, OutcomeVisitor)
            .await
            .unwrap();

        assert!(!outcome.keep);
        assert_eq!(outcome.steps.len(), 1);
        assert_eq!(outcome.dropped_by, Some(filter));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn visitor_layers_can_modify_control_flow() {
        let temp_dir = temp_dir();
        let filter = temp_dir.join("filter.rego");
        let enrich = temp_dir.join("enrich.rego");
        fs::write(
            &filter,
            "package ghrg.repos\n\ndefault allow := false\n\nallow if { input.archived == false }\noutput := input\n",
        )
        .unwrap();
        fs::write(
            &enrich,
            "# ```ghrg\n# contexts:\n#   - type: commits\n#     params:\n#       limit: 1\n# ```\n\npackage ghrg.repos\n\ndefault allow := true\noutput := {\"name\": input.name, \"team\": input.team}\n",
        )
        .unwrap();

        let input = serde_json::json!({"name": "api", "team": "platform", "archived": true});
        let mut engine = Engine::new();
        engine.push_file(filter).unwrap();
        engine.push_file(enrich).unwrap();
        let engine = engine.finish().unwrap();
        let visitor = ForceKeepLayer.layer(OutcomeVisitor);
        let outcome = engine.run(&input, &EmptyResolver, visitor).await.unwrap();

        assert!(outcome.keep);
        assert_eq!(outcome.steps.len(), 2);
        assert_eq!(outcome.steps[1].package, "data.ghrg.repos");
        assert_eq!(
            outcome.steps[1]
                .metadata
                .as_ref()
                .unwrap()
                .metadata
                .contexts
                .len(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn lazy_context_resolution_stops_after_drop() {
        let temp_dir = temp_dir();
        let filter = temp_dir.join("filter.rego");
        let enrich = temp_dir.join("enrich.rego");
        fs::write(
            &filter,
            "package ghrg.repos\n\ndefault allow := false\noutput := input\n",
        )
        .unwrap();
        fs::write(
            &enrich,
            "# ```ghrg\n# contexts:\n#   - type: languages\n# ```\n\npackage ghrg.repos\n\ndefault allow := true\noutput := input.contexts.languages\n",
        )
        .unwrap();

        let input = serde_json::json!({"name": "api"});
        let mut engine = Engine::new();
        engine.push_file(filter).unwrap();
        engine.push_file(enrich).unwrap();
        let engine = engine.finish().unwrap();
        let resolver = RecordingResolver::default();

        let outcome = engine.run(&input, &resolver, OutcomeVisitor).await.unwrap();

        assert!(!outcome.keep);
        assert_eq!(outcome.steps.len(), 1);
        assert!(resolver.calls.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reuses_cached_contexts_across_policies() {
        let temp_dir = temp_dir();
        let first = temp_dir.join("first.rego");
        let second = temp_dir.join("second.rego");
        fs::write(
            &first,
            "# ```ghrg\n# contexts:\n#   - name: first_context\n#     type: languages\n# ```\n\npackage ghrg.repos\n\ndefault allow := true\noutput := {\"first\": input.contexts.first_context}\n",
        )
        .unwrap();
        fs::write(
            &second,
            "# ```ghrg\n# contexts:\n#   - name: second_context\n#     type: languages\n# ```\n\npackage ghrg.repos\n\ndefault allow := true\noutput := {\"second\": input.contexts.second_context}\n",
        )
        .unwrap();

        let input = serde_json::json!({"name": "api"});
        let mut engine = Engine::new();
        engine.push_file(first).unwrap();
        engine.push_file(second).unwrap();
        let engine = engine.finish().unwrap();
        let resolver = RecordingResolver::default();

        let outcome = engine.run(&input, &resolver, OutcomeVisitor).await.unwrap();
        let calls = resolver.calls.lock().unwrap().clone();

        assert!(outcome.keep);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], vec!["languages".to_string()]);
        assert_eq!(
            outcome.result.object.field("second"),
            Some(&Value::String("value:languages".to_string()))
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn allows_same_context_key_with_different_specs_across_policies() {
        let temp_dir = temp_dir();
        let first = temp_dir.join("first.rego");
        let second = temp_dir.join("second.rego");
        fs::write(
            &first,
            "# ```ghrg\n# contexts:\n#   - name: shared\n#     type: commits\n#     params:\n#       limit: 1\n# ```\n\npackage ghrg.repos\n\ndefault allow := true\noutput := {\"first\": input.contexts.shared}\n",
        )
        .unwrap();
        fs::write(
            &second,
            "# ```ghrg\n# contexts:\n#   - name: shared\n#     type: commits\n#     params:\n#       limit: 2\n# ```\n\npackage ghrg.repos\n\ndefault allow := true\noutput := {\"second\": input.contexts.shared}\n",
        )
        .unwrap();

        let input = serde_json::json!({"name": "api"});
        let mut engine = Engine::new();
        engine.push_file(first).unwrap();
        engine.push_file(second).unwrap();
        let engine = engine.finish().unwrap();
        let resolver = RecordingResolver::default();

        let outcome = engine.run(&input, &resolver, OutcomeVisitor).await.unwrap();
        let calls = resolver.calls.lock().unwrap().clone();

        assert!(outcome.keep);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], vec!["commits(limit=1)".to_string()]);
        assert_eq!(calls[1], vec!["commits(limit=2)".to_string()]);
        assert_eq!(
            outcome.result.object.field("second"),
            Some(&Value::String("value:commits(limit=2)".to_string()))
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolves_dynamic_contexts_from_input_env_and_meta() {
        let temp_dir = temp_dir();
        let seed = temp_dir.join("seed.rego");
        let consume = temp_dir.join("consume.rego");

        fs::write(
            &seed,
            "# ```ghrg\n# name: seed_meta\n# contexts: []\n# ```\n\npackage ghrg.repos\n\ndefault allow := true\noutput := input\nmeta := {\"commit_limit\": 2}\n",
        )
        .unwrap();
        fs::write(
            &consume,
            "# ```ghrg\n# contexts:\n#   - name: recent\n#     type: commits\n#     params:\n#       limit:\n#         from: meta.policies.seed_meta.commit_limit\n#       author:\n#         from: env.PATH\n#       ref:\n#         from: input.default_branch\n# ```\n\npackage ghrg.repos\n\ndefault allow := true\noutput := {\"recent\": input.contexts.recent}\n",
        )
        .unwrap();

        let input = serde_json::json!({"default_branch": "main"});
        let mut engine = Engine::new();
        engine.push_file(seed).unwrap();
        engine.push_file(consume).unwrap();
        let engine = engine.finish().unwrap();
        let resolver = RecordingResolver::default();

        let outcome = engine.run(&input, &resolver, OutcomeVisitor).await.unwrap();
        let calls = resolver.calls.lock().unwrap().clone();

        assert!(outcome.keep);
        assert_eq!(calls.len(), 1);
        assert!(calls[0][0].contains("commits(limit=2"));
        assert!(calls[0][0].contains("ref=main"));
        assert!(calls[0][0].contains("author="));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dynamic_context_resolution_fails_when_source_missing_without_default() {
        let temp_dir = temp_dir();
        let policy = temp_dir.join("dynamic-missing.rego");

        fs::write(
            &policy,
            "# ```ghrg\n# contexts:\n#   - type: commits\n#     params:\n#       limit:\n#         from: input.missing_limit\n# ```\n\npackage ghrg.repos\n\ndefault allow := true\noutput := input\n",
        )
        .unwrap();

        let input = serde_json::json!({"name": "api"});
        let mut engine = Engine::new();
        engine.push_file(policy).unwrap();
        let engine = engine.finish().unwrap();
        let resolver = RecordingResolver::default();

        let error = engine
            .run(&input, &resolver, OutcomeVisitor)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            GhrgError::ContextDynamicSourceMissing { .. }
        ));
        assert!(resolver.calls.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dynamic_context_resolution_uses_default_when_source_missing() {
        let temp_dir = temp_dir();
        let policy = temp_dir.join("dynamic-default.rego");

        fs::write(
            &policy,
            "# ```ghrg\n# contexts:\n#   - type: commits\n#     params:\n#       limit:\n#         from: input.missing_limit\n#         default: 4\n# ```\n\npackage ghrg.repos\n\ndefault allow := true\noutput := input\n",
        )
        .unwrap();

        let input = serde_json::json!({"name": "api"});
        let mut engine = Engine::new();
        engine.push_file(policy).unwrap();
        let engine = engine.finish().unwrap();
        let resolver = RecordingResolver::default();

        let outcome = engine.run(&input, &resolver, OutcomeVisitor).await.unwrap();
        let calls = resolver.calls.lock().unwrap().clone();

        assert!(outcome.keep);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], vec!["commits(limit=4)".to_string()]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dynamic_context_resolution_parses_scalar_from_env_string() {
        let temp_dir = temp_dir();
        let policy = temp_dir.join("dynamic-env-limit.rego");

        fs::write(
            &policy,
            "# ```ghrg\n# contexts:\n#   - type: commits\n#     params:\n#       limit:\n#         from: env.GHRG_TEST_DYNAMIC_LIMIT\n# ```\n\npackage ghrg.repos\n\ndefault allow := true\noutput := input\n",
        )
        .unwrap();

        let input = serde_json::json!({"name": "api"});
        let mut engine = Engine::new();
        engine.push_file(policy).unwrap();
        let engine = engine.finish().unwrap();
        let resolver = RecordingResolver::default();

        let _lock = ENV_TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        unsafe {
            std::env::set_var("GHRG_TEST_DYNAMIC_LIMIT", "7");
        }

        let outcome = engine.run(&input, &resolver, OutcomeVisitor).await.unwrap();
        let calls = resolver.calls.lock().unwrap().clone();

        unsafe {
            std::env::remove_var("GHRG_TEST_DYNAMIC_LIMIT");
        }

        assert!(outcome.keep);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], vec!["commits(limit=7)".to_string()]);
    }

    #[test]
    fn engine_rejects_unknown_context_provider() {
        let temp_dir = temp_dir();
        let policy = temp_dir.join("sample.rego");
        fs::write(
            &policy,
            "# ```ghrg\n# contexts:\n#   - type: mystery\n# ```\n\npackage ghrg.repos\n",
        )
        .unwrap();

        let mut engine = Engine::new();
        let error = engine.push_file(&policy).unwrap_err();
        assert!(error.to_string().contains("mystery"));
    }

    #[test]
    fn engine_accepts_files_context_provider() {
        let temp_dir = temp_dir();
        let policy = temp_dir.join("sample.rego");
        fs::write(
            &policy,
            "# ```ghrg\n# contexts:\n#   - type: files\n#     params:\n#       glob: src/**\n#       limit: 25\n#       ref: main\n# ```\n\npackage ghrg.repos\n",
        )
        .unwrap();

        let mut engine = Engine::new();
        engine.push_file(&policy).unwrap();
    }

    #[test]
    fn engine_rejects_unknown_files_context_field() {
        let temp_dir = temp_dir();
        let policy = temp_dir.join("sample.rego");
        fs::write(
            &policy,
            "# ```ghrg\n# contexts:\n#   - type: files\n#     params:\n#       mode: bad\n# ```\n\npackage ghrg.repos\n",
        )
        .unwrap();

        let mut engine = Engine::new();
        let error = engine.push_file(&policy).unwrap_err();
        assert!(error.to_string().contains("mode"));
    }

    #[test]
    fn policy_inspect_rejects_unknown_context_provider_without_strict_mode() {
        let temp_dir = temp_dir();
        let policy = temp_dir.join("sample.rego");
        fs::write(
            &policy,
            "# ```ghrg\n# contexts:\n#   - type: mystery\n# ```\n\npackage ghrg.repos\n",
        )
        .unwrap();

        let mut engine = Engine::new();
        let error = engine.push_file(&policy).unwrap_err();
        assert!(error.to_string().contains("mystery"));
    }

    #[test]
    fn engine_accepts_languages_context_provider() {
        let temp_dir = temp_dir();
        let policy = temp_dir.join("sample.rego");
        fs::write(
            &policy,
            "# ```ghrg\n# contexts:\n#   - type: languages\n# ```\n\npackage ghrg.repos\n\ndefault allow := true\noutput := input\n",
        )
        .unwrap();

        let mut engine = Engine::new();
        engine.push_file(&policy).unwrap();
    }

    #[test]
    fn engine_accepts_branches_context_provider() {
        let temp_dir = temp_dir();
        let policy = temp_dir.join("sample.rego");
        fs::write(
            &policy,
            "# ```ghrg\n# contexts:\n#   - type: branches\n#     params:\n#       limit: 25\n#       protected: true\n# ```\n\npackage ghrg.repos\n\ndefault allow := true\noutput := input\n",
        )
        .unwrap();

        let mut engine = Engine::new();
        engine.push_file(&policy).unwrap();
    }

    #[test]
    fn engine_accepts_contributors_context_provider() {
        let temp_dir = temp_dir();
        let policy = temp_dir.join("sample.rego");
        fs::write(
            &policy,
            "# ```ghrg\n# contexts:\n#   - type: contributors\n#     params:\n#       limit: 25\n#       anonymous: true\n# ```\n\npackage ghrg.repos\n\ndefault allow := true\noutput := input\n",
        )
        .unwrap();

        let mut engine = Engine::new();
        engine.push_file(&policy).unwrap();
    }

    #[test]
    fn engine_accepts_workflow_runs_context_provider() {
        let temp_dir = temp_dir();
        let policy = temp_dir.join("sample.rego");
        fs::write(
            &policy,
            "# ```ghrg\n# contexts:\n#   - type: workflow_runs\n#     params:\n#       limit: 5\n#       branch: main\n#       status: completed\n# ```\n\npackage ghrg.repos\n\ndefault allow := true\noutput := input\n",
        )
        .unwrap();

        let mut engine = Engine::new();
        engine.push_file(&policy).unwrap();
    }

    #[test]
    fn policy_evaluation_uses_regorus_line_and_column_for_span() {
        let source = PolicySource {
            path: PathBuf::from("bad-eval.rego"),
            package: Some("ghrg.repos".to_string()),
            source: "package ghrg.repos\n\ndefault allow := false\n\nallow if { 1 / 0 > 0 }\n"
                .to_string(),
        };

        let error = source.evaluation_error(
            "data.ghrg.repos.allow",
            "--> bad-eval.rego:5:14\n  |\n5 | allow if { 1 / 0 > 0 }\n  |              ^\nerror: divide by zero"
                .to_string(),
        );

        match error {
            GhrgError::PolicyEvaluation { span, .. } => {
                assert_eq!(span.offset(), 57);
            }
            _ => panic!("expected policy evaluation error"),
        }
    }

    #[test]
    fn embedded_metadata_yaml_error_span_handles_crlf_newlines() {
        let source = PolicySource {
            path: PathBuf::from("bad-frontmatter.rego"),
            package: Some("ghrg.repos".to_string()),
            source: "# ```ghrg\r\n# name: bad\r\n# contexts:\r\n#   - type: commits\r\n#     params:\r\n#       limit: [\r\n# ```\r\n\r\npackage ghrg.repos\r\n"
                .to_string(),
        };

        let error = load_policy_metadata(&source).unwrap_err();

        match error {
            GhrgError::EmbeddedMetadataYaml { span, .. } => {
                assert_eq!(
                    span.offset(),
                    source.source.find("# ```\r\n\r\npackage").unwrap()
                );
            }
            _ => panic!("expected embedded metadata YAML error"),
        }
    }

    fn temp_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ghrg-test-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    static ENV_TEST_LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
}
