use comfy_table::{
    Attribute, Cell, Color as TableColor, ContentArrangement, Row, Table,
    modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL,
};
use console::style;
use ghrg_core::error::{GhrgError, Result};
use ghrg_core::policy::{OutputObject, PolicyResult};
use serde::Serialize;
use serde_json::Value;
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Pretty,
    Json,
    Csv,
    Raw,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrettyFormatOptions {
    pub group_by: Option<String>,
    pub sort_by: Option<String>,
}

pub trait OutputFormatter {
    fn format(&self, output: &CommandOutput) -> Result<String>;
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandOutput {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub data: OutputData,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "value")]
pub enum OutputData {
    Record(OutputRecord),
    Collection(Vec<OutputRecord>),
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct OutputRecord {
    pub object: OutputObject,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
struct RawEnvelope<'a> {
    command: &'a str,
    title: &'a Option<String>,
    data: &'a OutputData,
    visible: Value,
}

impl CommandOutput {
    pub fn record(
        command: impl Into<String>,
        title: impl Into<Option<String>>,
        record: OutputRecord,
    ) -> Self {
        Self {
            command: command.into(),
            title: title.into(),
            data: OutputData::Record(record),
        }
    }

    pub fn collection(
        command: impl Into<String>,
        title: impl Into<Option<String>>,
        records: Vec<OutputRecord>,
    ) -> Self {
        Self {
            command: command.into(),
            title: title.into(),
            data: OutputData::Collection(records),
        }
    }

    pub fn format(&self, format: OutputFormat) -> Result<String> {
        match format {
            OutputFormat::Pretty => self.format_pretty(PrettyFormatOptions::default()),
            OutputFormat::Json => JsonFormatter.format(self),
            OutputFormat::Csv => CsvFormatter.format(self),
            OutputFormat::Raw => RawFormatter.format(self),
        }
    }

    pub fn format_pretty(&self, options: PrettyFormatOptions) -> Result<String> {
        PrettyFormatter { options }.format(self)
    }

    pub fn visible_records(&self) -> Vec<&OutputRecord> {
        match &self.data {
            OutputData::Record(record) => vec![record],
            OutputData::Collection(records) => records.iter().collect(),
        }
    }

    pub fn visible_value(&self) -> Value {
        match &self.data {
            OutputData::Record(record) => record.object.json_value(),
            OutputData::Collection(records) => Value::Array(
                records
                    .iter()
                    .map(|record| record.object.json_value())
                    .collect(),
            ),
        }
    }
}

impl OutputRecord {
    pub fn from_object(object: OutputObject) -> Self {
        Self { object, meta: None }
    }

    pub fn with_meta(mut self, meta: impl Into<Value>) -> Self {
        self.meta = Some(meta.into());
        self
    }

    pub fn from_serializable<T: Serialize>(value: &T) -> Result<Self> {
        Ok(Self::from_object(OutputObject::from_serializable(value)?))
    }
}

impl From<PolicyResult> for OutputRecord {
    fn from(value: PolicyResult) -> Self {
        Self {
            object: value.object,
            meta: value.meta,
        }
    }
}

pub struct PrettyFormatter {
    options: PrettyFormatOptions,
}
pub struct JsonFormatter;
pub struct CsvFormatter;
pub struct RawFormatter;

impl OutputFormatter for PrettyFormatter {
    fn format(&self, output: &CommandOutput) -> Result<String> {
        let mut records = output.visible_records();

        if let Some(sort_by) = self.options.sort_by.as_deref() {
            sort_records(&mut records, sort_by);
        }

        Ok(match &output.data {
            OutputData::Collection(_) => {
                if let Some(group_by) = self.options.group_by.as_deref() {
                    render_grouped_collection(
                        output,
                        &records,
                        group_by,
                        self.options.sort_by.as_deref(),
                    )
                } else if records
                    .iter()
                    .all(|record| is_scalar_record(&record.object))
                {
                    render_collection_table(output, &records)
                } else {
                    render_collection_blocks(output, &records)
                }
            }
            OutputData::Record(record) => {
                render_record_block(output.title.as_deref(), &record.object)
            }
        })
    }
}

impl OutputFormatter for JsonFormatter {
    fn format(&self, output: &CommandOutput) -> Result<String> {
        Ok(serde_json::to_string_pretty(&output.visible_value())?)
    }
}

impl OutputFormatter for CsvFormatter {
    fn format(&self, output: &CommandOutput) -> Result<String> {
        let rows = output.visible_records();
        if rows.iter().any(|record| {
            record
                .object
                .fields
                .iter()
                .any(|field| is_nested(&field.value))
        }) {
            return Err(GhrgError::CsvRequiresScalarFields);
        }

        let headers = csv_headers(&rows);
        let mut writer = csv::Writer::from_writer(vec![]);
        writer.write_record(&headers)?;

        for record in rows {
            let row = headers
                .iter()
                .map(|header| {
                    record
                        .object
                        .field(header)
                        .map(display_value)
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>();
            writer.write_record(row)?;
        }

        let bytes = writer
            .into_inner()
            .map_err(|error| GhrgError::Csv(csv::Error::from(error.into_error())))?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

impl OutputFormatter for RawFormatter {
    fn format(&self, output: &CommandOutput) -> Result<String> {
        let envelope = RawEnvelope {
            command: &output.command,
            title: &output.title,
            data: &output.data,
            visible: output.visible_value(),
        };
        Ok(serde_json::to_string_pretty(&envelope)?)
    }
}

fn display_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(value).unwrap_or_else(|_| "<invalid json>".to_string())
        }
    }
}

fn display_styled_value(value: &Value) -> String {
    match value {
        Value::Null => style("null").dim().to_string(),
        Value::Bool(true) => style("true").green().bold().to_string(),
        Value::Bool(false) => style("false").yellow().to_string(),
        Value::Number(number) => style(number).cyan().to_string(),
        Value::String(text) => text.clone(),
        Value::Array(_) | Value::Object(_) => pretty_json(value),
    }
}

fn pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "<invalid json>".to_string())
}

fn pretty_field_name(name: &str) -> String {
    name.replace(['_', '-'], " ")
}

fn title_line(title: &str) -> String {
    style(title).bold().underlined().to_string()
}

fn render_record_block(title: Option<&str>, object: &OutputObject) -> String {
    let mut lines = Vec::new();
    if let Some(title) = title {
        lines.push(title_line(title));
    }
    lines.push(render_kv_table(object));
    lines.join("\n\n")
}

fn render_collection_blocks(output: &CommandOutput, records: &[&OutputRecord]) -> String {
    let mut lines = Vec::new();

    if let Some(title) = &output.title {
        lines.push(title_line(title));
        lines.push(
            style(format!("{} records", records.len()))
                .dim()
                .to_string(),
        );
    }

    if !records.is_empty() {
        lines.push(String::new());
    }

    lines.push(render_records_as_blocks(records));

    lines.join("\n")
}

fn render_records_as_blocks(records: &[&OutputRecord]) -> String {
    let mut lines = Vec::new();

    for (index, record) in records.iter().enumerate() {
        if index > 0 {
            lines.push(String::new());
        }
        lines.push(
            style(format!("Record {}", index + 1))
                .bold()
                .blue()
                .to_string(),
        );
        lines.push(render_kv_table(&record.object));
    }

    lines.join("\n")
}

fn render_collection_table(output: &CommandOutput, records: &[&OutputRecord]) -> String {
    let mut lines = Vec::new();

    if let Some(title) = &output.title {
        lines.push(title_line(title));
        lines.push(
            style(format!("{} records", records.len()))
                .dim()
                .to_string(),
        );
        lines.push(String::new());
    }

    lines.push(render_scalar_table(records));
    lines.join("\n")
}

fn render_grouped_collection(
    output: &CommandOutput,
    records: &[&OutputRecord],
    group_by: &str,
    sort_by: Option<&str>,
) -> String {
    let mut lines = Vec::new();

    if let Some(title) = &output.title {
        lines.push(title_line(title));
        lines.push(
            style(format!("{} records", records.len()))
                .dim()
                .to_string(),
        );
    }

    let mut groups = group_records(records, group_by);
    groups.sort_by(|left, right| compare_values(&left.key, &right.key));

    if !groups.is_empty() {
        lines.push(String::new());
    }

    for (index, group) in groups.iter_mut().enumerate() {
        if let Some(sort_by) = sort_by {
            sort_records(&mut group.records, sort_by);
        }

        if index > 0 {
            lines.push(String::new());
        }

        lines.push(render_group_heading(
            group_by,
            &group.key,
            group.records.len(),
        ));
        lines.push(render_group_body(&group.records));
    }

    lines.join("\n")
}

fn render_group_heading(field: &str, key: &Value, count: usize) -> String {
    format!(
        "{} {}",
        style(format!(
            "{}: {}",
            pretty_field_name(field),
            display_value(key)
        ))
        .bold()
        .blue(),
        style(format!(
            "({count} record{})",
            if count == 1 { "" } else { "s" }
        ))
        .dim(),
    )
}

fn render_group_body(records: &[&OutputRecord]) -> String {
    if records
        .iter()
        .all(|record| is_scalar_record(&record.object))
    {
        render_scalar_table(records)
    } else {
        render_records_as_blocks(records)
    }
}

fn render_scalar_table(records: &[&OutputRecord]) -> String {
    let mut table = base_table();
    let headers = csv_headers(records);
    table.set_header(headers.iter().map(|header| {
        Cell::new(pretty_field_name(header))
            .fg(TableColor::Cyan)
            .add_attribute(Attribute::Bold)
    }));

    for record in records {
        table.add_row(headers.iter().map(|header| {
            let value = record.object.field(header).cloned().unwrap_or(Value::Null);
            scalar_cell(&value)
        }));
    }

    table.to_string()
}

fn render_kv_table(object: &OutputObject) -> String {
    let mut table = base_table();
    table.set_header(vec![
        Cell::new("Field")
            .fg(TableColor::Cyan)
            .add_attribute(Attribute::Bold),
        Cell::new("Value")
            .fg(TableColor::Cyan)
            .add_attribute(Attribute::Bold),
    ]);

    for field in &object.fields {
        table.add_row(Row::from(vec![
            key_cell(&pretty_field_name(&field.name)),
            detailed_cell(&field.value),
        ]));
    }

    table.to_string()
}

fn base_table() -> Table {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table
}

fn scalar_cell(value: &Value) -> Cell {
    match value {
        Value::Null => Cell::new("null").fg(TableColor::DarkGrey),
        Value::Bool(true) => Cell::new("true").fg(TableColor::Green),
        Value::Bool(false) => Cell::new("false").fg(TableColor::Yellow),
        Value::Number(number) => Cell::new(number.to_string()).fg(TableColor::Cyan),
        Value::String(text) => Cell::new(text),
        Value::Array(_) | Value::Object(_) => Cell::new(pretty_json(value)),
    }
}

fn detailed_cell(value: &Value) -> Cell {
    match value {
        Value::Array(_) | Value::Object(_) => Cell::new(display_styled_value(value)),
        _ => scalar_cell(value),
    }
}

fn key_cell(label: &str) -> Cell {
    Cell::new(label).add_attribute(Attribute::Bold)
}

fn is_scalar_record(object: &OutputObject) -> bool {
    object.fields.iter().all(|field| !is_nested(&field.value))
}

fn is_nested(value: &Value) -> bool {
    matches!(value, Value::Array(_) | Value::Object(_))
}

#[derive(Debug)]
struct RecordGroup<'a> {
    key: Value,
    records: Vec<&'a OutputRecord>,
}

fn group_records<'a>(records: &[&'a OutputRecord], field: &str) -> Vec<RecordGroup<'a>> {
    let mut groups = Vec::<RecordGroup<'a>>::new();

    for record in records {
        let value = record.object.field(field).cloned().unwrap_or(Value::Null);
        if let Some(group) = groups
            .iter_mut()
            .find(|group| compare_values(&group.key, &value) == Ordering::Equal)
        {
            group.records.push(record);
        } else {
            groups.push(RecordGroup {
                key: value,
                records: vec![record],
            });
        }
    }

    groups
}

fn sort_records(records: &mut Vec<&OutputRecord>, field: &str) {
    records.sort_by(|left, right| {
        let left = left.object.field(field).cloned().unwrap_or(Value::Null);
        let right = right.object.field(field).cloned().unwrap_or(Value::Null);
        compare_values(&left, &right)
    });
}

fn compare_values(left: &Value, right: &Value) -> Ordering {
    match (left, right) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,
        (Value::Bool(left), Value::Bool(right)) => left.cmp(right),
        (Value::Bool(_), _) => Ordering::Less,
        (_, Value::Bool(_)) => Ordering::Greater,
        (Value::Number(left), Value::Number(right)) => left
            .as_f64()
            .and_then(|left| right.as_f64().and_then(|right| left.partial_cmp(&right)))
            .unwrap_or_else(|| left.to_string().cmp(&right.to_string())),
        (Value::Number(_), _) => Ordering::Less,
        (_, Value::Number(_)) => Ordering::Greater,
        (Value::String(left), Value::String(right)) => left.cmp(right),
        (Value::String(_), _) => Ordering::Less,
        (_, Value::String(_)) => Ordering::Greater,
        _ => serde_json::to_string(left)
            .unwrap_or_default()
            .cmp(&serde_json::to_string(right).unwrap_or_default()),
    }
}

fn csv_headers(records: &[&OutputRecord]) -> Vec<String> {
    let mut headers = Vec::new();
    for record in records {
        for field in &record.object.fields {
            if !headers.iter().any(|header| header == &field.name) {
                headers.push(field.name.clone());
            }
        }
    }
    headers
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghrg_core::policy::OutputField;
    use serde_json::json;

    fn sample_record() -> OutputRecord {
        OutputRecord::from_object(OutputObject::new(vec![
            OutputField::new("name", "api"),
            OutputField::new("team", "platform"),
            OutputField::new("stars", 10),
        ]))
    }

    #[test]
    fn json_uses_only_final_visible_object() {
        let output = CommandOutput::record(
            "repos",
            Some("Repository".to_string()),
            sample_record().with_meta(json!({"policy": {"matched": true}})),
        );

        let rendered = output.format(OutputFormat::Json).unwrap();
        assert!(rendered.contains("\"name\": \"api\""));
        assert!(!rendered.contains("matched"));
    }

    #[test]
    fn raw_keeps_meta_for_traceability() {
        let output = CommandOutput::record(
            "repos",
            Some("Repository".to_string()),
            OutputRecord::from(
                PolicyResult::new(sample_record().object)
                    .with_meta(json!({"policy": {"matched": true}})),
            ),
        );

        let rendered = output.format(OutputFormat::Raw).unwrap();
        assert!(rendered.contains("\"meta\""));
        assert!(rendered.contains("matched"));
        assert!(rendered.contains("\"visible\""));
    }

    #[test]
    fn csv_rejects_nested_values_in_final_output() {
        let output = CommandOutput::record(
            "repos",
            Some("Repo".to_string()),
            OutputRecord::from_object(OutputObject::new(vec![OutputField::new(
                "meta",
                json!({"team": "platform"}),
            )])),
        );

        let error = output.format(OutputFormat::Csv).unwrap_err();
        assert!(matches!(error, GhrgError::CsvRequiresScalarFields));
    }

    #[test]
    fn csv_preserves_policy_field_order_for_headers() {
        let output = CommandOutput::record(
            "repos",
            Some("Repo".to_string()),
            OutputRecord::from_object(OutputObject::new(vec![
                OutputField::new("stars", 10),
                OutputField::new("name", "api"),
            ])),
        );

        let rendered = output.format(OutputFormat::Csv).unwrap();
        assert!(rendered.starts_with("stars,name\n"));
    }

    #[test]
    fn policy_result_maps_directly_to_output_record() {
        let output = CommandOutput::record(
            "repos",
            Some("Repository".to_string()),
            OutputRecord::from(
                PolicyResult::from_serializable(
                    &serde_json::json!({"name": "api", "team": "platform"}),
                )
                .unwrap()
                .with_meta(json!({"source": "policy"})),
            ),
        );

        let rendered = output.format(OutputFormat::Json).unwrap();
        assert!(rendered.contains("\"name\": \"api\""));
        assert!(!rendered.contains("source"));
    }

    #[test]
    fn output_object_from_struct_preserves_serialized_fields() {
        #[derive(Serialize)]
        struct RepoSummary {
            name: &'static str,
            archived: bool,
        }

        let object = OutputObject::from_serializable(&RepoSummary {
            name: "api",
            archived: false,
        })
        .unwrap();

        assert_eq!(object.field_names(), vec!["archived", "name"]);
        assert_eq!(
            object.field("name"),
            Some(&Value::String("api".to_string()))
        );
    }

    #[test]
    fn output_object_rejects_non_object_json_values() {
        let error = OutputObject::from_serializable(&vec!["api", "web"]).unwrap_err();
        assert!(matches!(error, GhrgError::OutputObjectRequiresJsonObject));
    }

    #[test]
    fn pretty_humanizes_field_names_and_expands_nested_values() {
        let output = CommandOutput::record(
            "policy test",
            Some("Policy Test".to_string()),
            OutputRecord::from_object(OutputObject::new(vec![
                OutputField::new("dropped_by", Value::Null),
                OutputField::new("final_output", json!({"name": "api"})),
            ])),
        );

        let rendered = output.format(OutputFormat::Pretty).unwrap();
        assert!(rendered.contains("dropped by"));
        assert!(rendered.contains("null"));
        assert!(rendered.contains("final output"));
        assert!(rendered.contains("\"name\": \"api\""));
    }

    #[test]
    fn pretty_sorts_scalar_collections_by_requested_field() {
        let output = CommandOutput::collection(
            "repos",
            Some("Repositories".to_string()),
            vec![
                OutputRecord::from_object(OutputObject::new(vec![
                    OutputField::new("name", "web"),
                    OutputField::new("stars", 20),
                ])),
                OutputRecord::from_object(OutputObject::new(vec![
                    OutputField::new("name", "api"),
                    OutputField::new("stars", 10),
                ])),
            ],
        );

        let rendered = output
            .format_pretty(PrettyFormatOptions {
                group_by: None,
                sort_by: Some("name".to_string()),
            })
            .unwrap();

        assert!(rendered.find("api").unwrap() < rendered.find("web").unwrap());
    }

    #[test]
    fn pretty_sort_treats_missing_fields_as_null() {
        let output = CommandOutput::collection(
            "repos",
            Some("Repositories".to_string()),
            vec![
                OutputRecord::from_object(OutputObject::new(vec![OutputField::new("name", "web")])),
                OutputRecord::from_object(OutputObject::new(vec![
                    OutputField::new("name", "api"),
                    OutputField::new("team", "platform"),
                ])),
            ],
        );

        let rendered = output
            .format_pretty(PrettyFormatOptions {
                group_by: None,
                sort_by: Some("team".to_string()),
            })
            .unwrap();

        assert!(rendered.find("web").unwrap() < rendered.find("api").unwrap());
    }

    #[test]
    fn pretty_groups_scalar_collections_by_requested_field() {
        let output = CommandOutput::collection(
            "repos",
            Some("Repositories".to_string()),
            vec![
                OutputRecord::from_object(OutputObject::new(vec![
                    OutputField::new("name", "api"),
                    OutputField::new("team", "platform"),
                ])),
                OutputRecord::from_object(OutputObject::new(vec![OutputField::new(
                    "name", "docs",
                )])),
            ],
        );

        let rendered = output
            .format_pretty(PrettyFormatOptions {
                group_by: Some("team".to_string()),
                sort_by: None,
            })
            .unwrap();

        assert!(rendered.contains("team: null"));
        assert!(rendered.contains("team: platform"));
    }

    #[test]
    fn pretty_groups_block_collections_and_sorts_within_group() {
        let output = CommandOutput::collection(
            "repos",
            Some("Repositories".to_string()),
            vec![
                OutputRecord::from_object(OutputObject::new(vec![
                    OutputField::new("name", "web"),
                    OutputField::new("team", "platform"),
                    OutputField::new("meta", json!({"lang": "rust"})),
                ])),
                OutputRecord::from_object(OutputObject::new(vec![
                    OutputField::new("name", "api"),
                    OutputField::new("team", "platform"),
                    OutputField::new("meta", json!({"lang": "go"})),
                ])),
            ],
        );

        let rendered = output
            .format_pretty(PrettyFormatOptions {
                group_by: Some("team".to_string()),
                sort_by: Some("name".to_string()),
            })
            .unwrap();

        assert!(rendered.contains("team: platform"));
        assert!(rendered.find("api").unwrap() < rendered.find("web").unwrap());
    }
}
