//! Exact JSON Schema to the bounded Google `Schema` vocabulary accepted by Code Assist.

use serde_json::{Map, Number, Value};

const MAX_TRANSLATED_NODES: usize = 4_096;
const MAX_REFERENCE_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SchemaError {
    path: String,
    detail: String,
}

impl SchemaError {
    fn new(path: &SchemaPath, detail: impl Into<String>) -> Self {
        Self {
            path: path.render(),
            detail: detail.into(),
        }
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn message(&self) -> String {
        format!("Invalid JSON Schema at {}: {}", self.path, self.detail)
    }
}

#[derive(Clone)]
struct SchemaPath {
    root: String,
    segments: Vec<String>,
}

impl SchemaPath {
    fn root(root: &str) -> Self {
        Self {
            root: root.to_string(),
            segments: Vec::new(),
        }
    }

    fn child(&self, segment: impl Into<String>) -> Self {
        let mut path = self.clone();
        path.segments.push(segment.into());
        path
    }

    fn render(&self) -> String {
        let mut rendered = self.root.clone();
        for segment in &self.segments {
            rendered.push('/');
            rendered.push_str(&segment.replace('~', "~0").replace('/', "~1"));
        }
        rendered
    }
}

pub(crate) fn translate(schema: &Value, root_path: &str) -> Result<Value, SchemaError> {
    let mut translator = Translator {
        root: schema,
        remaining_nodes: MAX_TRANSLATED_NODES,
        active_references: Vec::new(),
    };
    translator.translate_node(schema, &SchemaPath::root(root_path), 0)
}

struct Translator<'a> {
    root: &'a Value,
    remaining_nodes: usize,
    active_references: Vec<String>,
}

impl Translator<'_> {
    fn translate_node(
        &mut self,
        schema: &Value,
        path: &SchemaPath,
        depth: usize,
    ) -> Result<Value, SchemaError> {
        if self.remaining_nodes == 0 {
            return Err(SchemaError::new(
                path,
                format!("expanded schema exceeds {MAX_TRANSLATED_NODES} nodes"),
            ));
        }
        self.remaining_nodes -= 1;
        if depth > MAX_REFERENCE_DEPTH {
            return Err(SchemaError::new(
                path,
                format!("schema nesting exceeds {MAX_REFERENCE_DEPTH}"),
            ));
        }

        let object = schema.as_object().ok_or_else(|| {
            SchemaError::new(path, "boolean and non-object schemas are not representable")
        })?;
        if object.contains_key("$ref") {
            return self.translate_reference(object, path, depth);
        }
        if depth > 0 && object.contains_key("$id") {
            return Err(SchemaError::new(
                &path.child("$id"),
                "nested $id changes reference scope and is not representable",
            ));
        }

        let mut output = Map::new();
        let type_allows_null = type_allows_null(object.get("type"));
        let mut schema_type = normalize_type(object.get("type"), path)?;
        let const_value = object.get("const");
        let enum_value = object.get("enum");
        if let Some(value) = const_value {
            if let Some(enum_value) = enum_value {
                let values = enum_value.as_array().ok_or_else(|| {
                    SchemaError::new(&path.child("enum"), "must be a non-empty array")
                })?;
                if values.is_empty() {
                    return Err(SchemaError::new(
                        &path.child("enum"),
                        "must be a non-empty array",
                    ));
                }
                if !values.iter().any(|candidate| candidate == value) {
                    return Err(SchemaError::new(
                        &path.child("const"),
                        "const and enum have an empty intersection",
                    ));
                }
            }
            apply_const(value, &mut schema_type, type_allows_null, &mut output, path)?;
        } else if let Some(value) = enum_value {
            apply_enum(value, &mut schema_type, type_allows_null, &mut output, path)?;
        }

        if let Some(schema_type) = schema_type.as_deref() {
            output.insert("type".to_string(), Value::String(schema_type.to_string()));
        }

        let explicit_nullable = optional_bool(object.get("nullable"), &path.child("nullable"))?;
        let requested_nullable = type_allows_null || explicit_nullable == Some(true);
        if const_value.is_none() && enum_value.is_none() {
            if requested_nullable {
                output.insert("nullable".to_string(), Value::Bool(true));
            } else if let Some(nullable) = explicit_nullable {
                output.insert("nullable".to_string(), Value::Bool(nullable));
            }
        } else if explicit_nullable == Some(false) {
            if schema_type.as_deref() == Some("null") {
                return Err(SchemaError::new(
                    &path.child("nullable"),
                    "nullable:false conflicts with a null-only const or enum",
                ));
            }
            output.insert("nullable".to_string(), Value::Bool(false));
        } else if schema_type.as_deref() != Some("null")
            && requested_nullable
            && !output
                .get("nullable")
                .is_some_and(|value| value == &Value::Bool(true))
        {
            // const/enum is an intersection and therefore cannot be widened by nullable.
            output.insert("nullable".to_string(), Value::Bool(false));
        }

        for keyword in ["title", "description", "format", "pattern"] {
            if let Some(value) = object.get(keyword) {
                let value = value
                    .as_str()
                    .ok_or_else(|| SchemaError::new(&path.child(keyword), "must be a string"))?;
                if keyword != "pattern" || schema_type.as_deref() == Some("string") {
                    output.insert(keyword.to_string(), Value::String(value.to_string()));
                }
            }
        }
        for keyword in ["default", "example"] {
            if let Some(value) = object.get(keyword) {
                output.insert(keyword.to_string(), value.clone());
            }
        }

        self.translate_structural_fields(object, &mut output, schema_type.as_deref(), path, depth)?;
        if schema_type.is_none() && !output.contains_key("anyOf") {
            return Err(SchemaError::new(
                path,
                "a schema without type or anyOf has no exact Code Assist representation",
            ));
        }
        translate_numeric_bounds(object, &mut output, schema_type.as_deref(), path)?;
        translate_cardinality_fields(object, &mut output, schema_type.as_deref(), path)?;
        reject_or_drop_unsupported(object, &mut output, schema_type.as_deref(), path)?;
        validate_cardinality_intersections(object, &output, schema_type.as_deref(), path)?;
        reject_unknown_keywords(object, path)?;

        if output.is_empty() {
            return Err(SchemaError::new(
                path,
                "an unconstrained schema has no exact Code Assist representation",
            ));
        }
        Ok(Value::Object(output))
    }

    fn translate_reference(
        &mut self,
        object: &Map<String, Value>,
        path: &SchemaPath,
        depth: usize,
    ) -> Result<Value, SchemaError> {
        let reference = object
            .get("$ref")
            .and_then(Value::as_str)
            .ok_or_else(|| SchemaError::new(&path.child("$ref"), "must be a string"))?;
        if depth > 0 && object.contains_key("$id") {
            return Err(SchemaError::new(
                &path.child("$id"),
                "nested $id changes reference scope and is not representable",
            ));
        }
        if !reference.starts_with("#/") {
            return Err(SchemaError::new(
                &path.child("$ref"),
                "only local JSON Pointer references are supported",
            ));
        }
        if self
            .active_references
            .iter()
            .any(|active| active == reference)
        {
            return Err(SchemaError::new(
                &path.child("$ref"),
                "recursive references are not representable",
            ));
        }
        for keyword in object.keys() {
            if !matches!(
                keyword.as_str(),
                "$ref"
                    | "$schema"
                    | "$id"
                    | "$comment"
                    | "$defs"
                    | "definitions"
                    | "title"
                    | "description"
                    | "default"
                    | "example"
                    | "examples"
            ) {
                return Err(SchemaError::new(
                    &path.child(keyword),
                    "$ref siblings with validation semantics are not representable",
                ));
            }
        }

        let pointer = &reference[1..];
        let target = self.root.pointer(pointer).cloned().ok_or_else(|| {
            SchemaError::new(&path.child("$ref"), "reference target does not exist")
        })?;
        let target_path = pointer_path(&SchemaPath::root(&path.root), pointer)?;
        self.active_references.push(reference.to_string());
        let translated = self.translate_node(&target, &target_path, depth + 1);
        self.active_references.pop();
        let mut translated = translated?;
        let translated_object = translated
            .as_object_mut()
            .expect("translator returns object");
        for keyword in ["title", "description"] {
            if let Some(value) = object.get(keyword) {
                let value = value
                    .as_str()
                    .ok_or_else(|| SchemaError::new(&path.child(keyword), "must be a string"))?;
                translated_object.insert(keyword.to_string(), Value::String(value.to_string()));
            }
        }
        for keyword in ["default", "example"] {
            if let Some(value) = object.get(keyword) {
                translated_object.insert(keyword.to_string(), value.clone());
            }
        }
        Ok(translated)
    }

    fn translate_structural_fields(
        &mut self,
        object: &Map<String, Value>,
        output: &mut Map<String, Value>,
        schema_type: Option<&str>,
        path: &SchemaPath,
        depth: usize,
    ) -> Result<(), SchemaError> {
        if let Some(properties) = object.get("properties") {
            if schema_type == Some("object") {
                let properties = properties.as_object().ok_or_else(|| {
                    SchemaError::new(&path.child("properties"), "must be an object")
                })?;
                let mut translated = Map::new();
                for (name, schema) in properties {
                    translated.insert(
                        name.clone(),
                        self.translate_node(
                            schema,
                            &path.child("properties").child(name),
                            depth + 1,
                        )?,
                    );
                }
                output.insert("properties".to_string(), Value::Object(translated));
            }
        }
        if let Some(items) = object.get("items") {
            if schema_type == Some("array") {
                output.insert(
                    "items".to_string(),
                    self.translate_node(items, &path.child("items"), depth + 1)?,
                );
            }
        }
        if let Some(any_of) = object.get("anyOf") {
            let any_of = any_of
                .as_array()
                .filter(|values| !values.is_empty())
                .ok_or_else(|| {
                    SchemaError::new(&path.child("anyOf"), "must be a non-empty array")
                })?;
            let mut translated = Vec::with_capacity(any_of.len());
            for (index, schema) in any_of.iter().enumerate() {
                translated.push(self.translate_node(
                    schema,
                    &path.child("anyOf").child(index.to_string()),
                    depth + 1,
                )?);
            }
            output.insert("anyOf".to_string(), Value::Array(translated));
        }
        Ok(())
    }
}

fn normalize_type(value: Option<&Value>, path: &SchemaPath) -> Result<Option<String>, SchemaError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let normalized = match value {
        Value::String(value) => normalize_type_name(value, &path.child("type"))?,
        Value::Array(values) => {
            let mut non_null = Vec::new();
            let mut has_null = false;
            for (index, value) in values.iter().enumerate() {
                let value = value.as_str().ok_or_else(|| {
                    SchemaError::new(
                        &path.child("type").child(index.to_string()),
                        "must be a string",
                    )
                })?;
                let value =
                    normalize_type_name(value, &path.child("type").child(index.to_string()))?;
                if value == "null" {
                    has_null = true;
                } else if !non_null.iter().any(|known| known == &value) {
                    non_null.push(value);
                }
            }
            if non_null.len() != 1 || !has_null {
                return Err(SchemaError::new(
                    &path.child("type"),
                    "only a single type optionally unioned with null is representable",
                ));
            }
            non_null.pop().unwrap()
        }
        _ => {
            return Err(SchemaError::new(
                &path.child("type"),
                "must be a string or array",
            ))
        }
    };
    Ok(Some(normalized))
}

fn type_allows_null(value: Option<&Value>) -> bool {
    value.and_then(Value::as_array).is_some_and(|values| {
        values.iter().any(|value| {
            value
                .as_str()
                .is_some_and(|value| value.eq_ignore_ascii_case("null"))
        })
    })
}

fn normalize_type_name(value: &str, path: &SchemaPath) -> Result<String, SchemaError> {
    let normalized = value.to_ascii_lowercase();
    if matches!(
        normalized.as_str(),
        "string" | "number" | "integer" | "boolean" | "array" | "object" | "null"
    ) {
        Ok(normalized)
    } else {
        Err(SchemaError::new(
            path,
            format!("unsupported type {value:?}"),
        ))
    }
}

fn apply_const(
    value: &Value,
    schema_type: &mut Option<String>,
    type_allows_null: bool,
    output: &mut Map<String, Value>,
    path: &SchemaPath,
) -> Result<(), SchemaError> {
    let inferred = value_type(value).ok_or_else(|| {
        SchemaError::new(
            &path.child("const"),
            "object and array const values are not representable",
        )
    })?;
    require_compatible_type(
        schema_type,
        inferred,
        type_allows_null,
        &path.child("const"),
    )?;
    *schema_type = Some(inferred.to_string());
    match value {
        Value::String(value) => {
            output.insert(
                "enum".to_string(),
                Value::Array(vec![Value::String(value.clone())]),
            );
        }
        Value::Number(value) => {
            let number = finite_number(value, &path.child("const"))?;
            let number = Number::from_f64(number).expect("finite");
            output.insert("minimum".to_string(), Value::Number(number.clone()));
            output.insert("maximum".to_string(), Value::Number(number));
        }
        Value::Null => {}
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => {
            return Err(SchemaError::new(
                &path.child("const"),
                "this const value is not representable",
            ))
        }
    }
    Ok(())
}

fn apply_enum(
    value: &Value,
    schema_type: &mut Option<String>,
    type_allows_null: bool,
    output: &mut Map<String, Value>,
    path: &SchemaPath,
) -> Result<(), SchemaError> {
    let values = value
        .as_array()
        .filter(|values| !values.is_empty())
        .ok_or_else(|| SchemaError::new(&path.child("enum"), "must be a non-empty array"))?;
    let strings: Vec<Value> = values
        .iter()
        .filter_map(|value| value.as_str().map(|value| Value::String(value.to_string())))
        .collect();
    let has_null = values.iter().any(Value::is_null);
    if strings.len() + usize::from(has_null) == values.len() {
        let effective_null = has_null
            && (schema_type.is_none()
                || schema_type.as_deref() == Some("null")
                || type_allows_null);
        let effective_strings = if schema_type
            .as_deref()
            .is_none_or(|declared| declared == "string")
        {
            strings
        } else {
            Vec::new()
        };
        if effective_strings.is_empty() && !effective_null {
            return Err(SchemaError::new(
                &path.child("enum"),
                "enum and the declared type have an empty intersection",
            ));
        }
        if effective_strings.is_empty() {
            *schema_type = Some("null".to_string());
        } else {
            *schema_type = Some("string".to_string());
            output.insert("enum".to_string(), Value::Array(effective_strings));
            if effective_null {
                output.insert("nullable".to_string(), Value::Bool(true));
            }
        }
        return Ok(());
    }
    if values.len() == 1 {
        let inferred = value_type(&values[0]).ok_or_else(|| {
            SchemaError::new(
                &path.child("enum").child("0"),
                "object and array enum values are not representable",
            )
        })?;
        require_compatible_type(
            schema_type,
            inferred,
            type_allows_null,
            &path.child("enum").child("0"),
        )?;
        match &values[0] {
            Value::Number(value) => {
                let number = finite_number(value, &path.child("enum").child("0"))?;
                let number = Number::from_f64(number).expect("finite");
                *schema_type = Some(inferred.to_string());
                output.insert("minimum".to_string(), Value::Number(number.clone()));
                output.insert("maximum".to_string(), Value::Number(number));
                return Ok(());
            }
            Value::Null => {
                *schema_type = Some("null".to_string());
                return Ok(());
            }
            _ => {}
        }
    }
    Err(SchemaError::new(
        &path.child("enum"),
        "only string enums, optionally including null, are representable",
    ))
}

fn require_compatible_type(
    schema_type: &Option<String>,
    inferred: &str,
    type_allows_null: bool,
    path: &SchemaPath,
) -> Result<(), SchemaError> {
    if schema_type.as_deref().is_none_or(|value| {
        value == inferred
            || (value == "number" && inferred == "integer")
            || (inferred == "null" && type_allows_null)
    }) {
        Ok(())
    } else {
        Err(SchemaError::new(
            path,
            "value conflicts with the declared type",
        ))
    }
}

fn value_type(value: &Value) -> Option<&'static str> {
    match value {
        Value::Null => Some("null"),
        Value::Bool(_) => Some("boolean"),
        Value::Number(value) if value.is_i64() || value.is_u64() => Some("integer"),
        Value::Number(_) => Some("number"),
        Value::String(_) => Some("string"),
        Value::Array(_) | Value::Object(_) => None,
    }
}

fn translate_numeric_bounds(
    object: &Map<String, Value>,
    output: &mut Map<String, Value>,
    schema_type: Option<&str>,
    path: &SchemaPath,
) -> Result<(), SchemaError> {
    if !matches!(schema_type, Some("number" | "integer")) {
        return Ok(());
    }
    let synthesized_keyword = if object.contains_key("const") {
        "const"
    } else {
        "enum"
    };
    let mut minimum = output
        .get("minimum")
        .and_then(Value::as_f64)
        .map(|value| NumericBound::new(value, synthesized_keyword));
    if let Some(value) = optional_number(object.get("minimum"), &path.child("minimum"))? {
        minimum = merge_numeric_bound(minimum, NumericBound::new(value, "minimum"), true);
    }
    let mut maximum = output
        .get("maximum")
        .and_then(Value::as_f64)
        .map(|value| NumericBound::new(value, synthesized_keyword));
    if let Some(value) = optional_number(object.get("maximum"), &path.child("maximum"))? {
        maximum = merge_numeric_bound(maximum, NumericBound::new(value, "maximum"), false);
    }
    if let Some(bound) = optional_number(
        object.get("exclusiveMinimum"),
        &path.child("exclusiveMinimum"),
    )? {
        let bound = next_up(bound).ok_or_else(|| {
            SchemaError::new(
                &path.child("exclusiveMinimum"),
                "exclusive bound has no finite inclusive successor",
            )
        })?;
        minimum = merge_numeric_bound(minimum, NumericBound::new(bound, "exclusiveMinimum"), true);
    }
    if let Some(bound) = optional_number(
        object.get("exclusiveMaximum"),
        &path.child("exclusiveMaximum"),
    )? {
        let bound = next_down(bound).ok_or_else(|| {
            SchemaError::new(
                &path.child("exclusiveMaximum"),
                "exclusive bound has no finite inclusive predecessor",
            )
        })?;
        maximum = merge_numeric_bound(maximum, NumericBound::new(bound, "exclusiveMaximum"), false);
    }
    if let Some(bound) = minimum {
        output.insert(
            "minimum".to_string(),
            Value::Number(Number::from_f64(bound.value).expect("finite")),
        );
    }
    if let Some(bound) = maximum {
        output.insert(
            "maximum".to_string(),
            Value::Number(Number::from_f64(bound.value).expect("finite")),
        );
    }
    if let Some((minimum, maximum)) = minimum
        .zip(maximum)
        .filter(|(minimum, maximum)| minimum.value > maximum.value)
    {
        let keyword = if maximum.keyword == synthesized_keyword {
            minimum.keyword
        } else {
            maximum.keyword
        };
        return Err(SchemaError::new(
            &path.child(keyword),
            "numeric constraints have an empty intersection",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct NumericBound {
    value: f64,
    keyword: &'static str,
}

impl NumericBound {
    fn new(value: f64, keyword: &'static str) -> Self {
        Self { value, keyword }
    }
}

fn merge_numeric_bound(
    current: Option<NumericBound>,
    candidate: NumericBound,
    lower: bool,
) -> Option<NumericBound> {
    match current {
        Some(current)
            if (lower && current.value >= candidate.value)
                || (!lower && current.value <= candidate.value) =>
        {
            Some(current)
        }
        _ => Some(candidate),
    }
}

fn translate_cardinality_fields(
    object: &Map<String, Value>,
    output: &mut Map<String, Value>,
    schema_type: Option<&str>,
    path: &SchemaPath,
) -> Result<(), SchemaError> {
    let fields: &[&str] = match schema_type {
        Some("string") => &["minLength", "maxLength"],
        Some("array") => &["minItems", "maxItems"],
        Some("object") => &["minProperties", "maxProperties"],
        _ => &[],
    };
    for field in fields {
        if let Some(value) = object.get(*field) {
            let value = value.as_u64().ok_or_else(|| {
                SchemaError::new(&path.child(*field), "must be a non-negative integer")
            })?;
            output.insert((*field).to_string(), Value::Number(value.into()));
        }
    }
    if schema_type == Some("object") {
        for field in ["required", "propertyOrdering"] {
            if let Some(value) = object.get(field) {
                output.insert(field.to_string(), string_array(value, &path.child(field))?);
            }
        }
    }
    Ok(())
}

fn validate_cardinality_intersections(
    object: &Map<String, Value>,
    output: &Map<String, Value>,
    schema_type: Option<&str>,
    path: &SchemaPath,
) -> Result<(), SchemaError> {
    let fields = match schema_type {
        Some("string") => Some(("minLength", "maxLength")),
        Some("array") => Some(("minItems", "maxItems")),
        Some("object") => Some(("minProperties", "maxProperties")),
        _ => None,
    };
    let Some((minimum_field, maximum_field)) = fields else {
        return Ok(());
    };
    let minimum = output.get(minimum_field).and_then(Value::as_u64);
    let maximum = output.get(maximum_field).and_then(Value::as_u64);
    if minimum
        .zip(maximum)
        .is_some_and(|(minimum, maximum)| minimum > maximum)
    {
        let source = if schema_type == Some("array")
            && object.get("maxContains").and_then(Value::as_u64)
                == output.get(maximum_field).and_then(Value::as_u64)
            && object.get("maxItems").and_then(Value::as_u64)
                != output.get(maximum_field).and_then(Value::as_u64)
        {
            "maxContains"
        } else {
            maximum_field
        };
        return Err(SchemaError::new(
            &path.child(source),
            "cardinality constraints have an empty intersection",
        ));
    }
    Ok(())
}

fn reject_or_drop_unsupported(
    object: &Map<String, Value>,
    output: &mut Map<String, Value>,
    schema_type: Option<&str>,
    path: &SchemaPath,
) -> Result<(), SchemaError> {
    if schema_type == Some("object") {
        if let Some(value) = object.get("additionalProperties") {
            if !is_true_schema(value) {
                return unsupported(path, "additionalProperties");
            }
        }
        if let Some(value) = object.get("unevaluatedProperties") {
            if !is_true_schema(value) {
                return unsupported(path, "unevaluatedProperties");
            }
        }
        if let Some(value) = object.get("patternProperties") {
            if !value.as_object().is_some_and(Map::is_empty) {
                return unsupported(path, "patternProperties");
            }
        }
        if let Some(value) = object.get("dependentRequired") {
            let no_op = value.as_object().is_some_and(|dependencies| {
                dependencies
                    .values()
                    .all(|required| required.as_array().is_some_and(Vec::is_empty))
            });
            if !no_op {
                return unsupported(path, "dependentRequired");
            }
        }
        if let Some(value) = object.get("propertyNames") {
            if !is_true_schema(value) {
                return unsupported(path, "propertyNames");
            }
        }
    }
    if schema_type == Some("array") {
        if let Some(value) = object.get("uniqueItems") {
            match value.as_bool() {
                Some(false) => {}
                Some(true) => return unsupported(path, "uniqueItems"),
                None => {
                    return Err(SchemaError::new(
                        &path.child("uniqueItems"),
                        "must be a boolean",
                    ))
                }
            }
        }
        if let Some(contains) = object.get("contains") {
            if !is_true_schema(contains) {
                return unsupported(path, "contains");
            }
            let minimum = object.get("minContains").map_or(Ok(1), |value| {
                value.as_u64().ok_or_else(|| {
                    SchemaError::new(&path.child("minContains"), "must be a non-negative integer")
                })
            })?;
            let maximum = object
                .get("maxContains")
                .map(|value| {
                    value.as_u64().ok_or_else(|| {
                        SchemaError::new(
                            &path.child("maxContains"),
                            "must be a non-negative integer",
                        )
                    })
                })
                .transpose()?;
            merge_u64_bound(output, "minItems", minimum, true);
            if let Some(maximum) = maximum {
                merge_u64_bound(output, "maxItems", maximum, false);
            }
        }
    }
    if matches!(schema_type, Some("number" | "integer")) {
        if let Some(value) = object.get("multipleOf") {
            let no_op = schema_type == Some("integer") && value.as_u64() == Some(1);
            if !no_op {
                return unsupported(path, "multipleOf");
            }
        }
    }
    for keyword in [
        "allOf",
        "oneOf",
        "not",
        "if",
        "then",
        "else",
        "dependentSchemas",
        "dependencies",
        "prefixItems",
        "additionalItems",
        "unevaluatedItems",
        "contentSchema",
        "$dynamicRef",
        "$recursiveRef",
    ] {
        if object.contains_key(keyword) {
            return unsupported(path, keyword);
        }
    }
    Ok(())
}

fn reject_unknown_keywords(
    object: &Map<String, Value>,
    path: &SchemaPath,
) -> Result<(), SchemaError> {
    for keyword in object.keys() {
        if !matches!(
            keyword.as_str(),
            "type"
                | "title"
                | "description"
                | "format"
                | "pattern"
                | "nullable"
                | "default"
                | "example"
                | "examples"
                | "enum"
                | "const"
                | "minimum"
                | "maximum"
                | "exclusiveMinimum"
                | "exclusiveMaximum"
                | "minLength"
                | "maxLength"
                | "minItems"
                | "maxItems"
                | "minProperties"
                | "maxProperties"
                | "properties"
                | "required"
                | "propertyOrdering"
                | "items"
                | "anyOf"
                | "$schema"
                | "$id"
                | "$anchor"
                | "$dynamicAnchor"
                | "$recursiveAnchor"
                | "$comment"
                | "deprecated"
                | "readOnly"
                | "writeOnly"
                | "$defs"
                | "definitions"
                | "additionalProperties"
                | "unevaluatedProperties"
                | "patternProperties"
                | "dependentRequired"
                | "propertyNames"
                | "contains"
                | "minContains"
                | "maxContains"
                | "uniqueItems"
                | "multipleOf"
                | "allOf"
                | "oneOf"
                | "not"
                | "if"
                | "then"
                | "else"
                | "dependentSchemas"
                | "dependencies"
                | "prefixItems"
                | "additionalItems"
                | "unevaluatedItems"
                | "contentEncoding"
                | "contentMediaType"
                | "contentSchema"
                | "$dynamicRef"
                | "$recursiveRef"
        ) && !keyword.starts_with("x-")
        {
            return Err(SchemaError::new(
                &path.child(keyword),
                format!("unknown keyword {keyword:?} is not allowed"),
            ));
        }
    }
    Ok(())
}

fn unsupported<T>(path: &SchemaPath, keyword: &str) -> Result<T, SchemaError> {
    Err(SchemaError::new(
        &path.child(keyword),
        format!("keyword {keyword:?} cannot be represented by Code Assist"),
    ))
}

fn optional_bool(value: Option<&Value>, path: &SchemaPath) -> Result<Option<bool>, SchemaError> {
    value
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| SchemaError::new(path, "must be a boolean"))
        })
        .transpose()
}

fn optional_number(value: Option<&Value>, path: &SchemaPath) -> Result<Option<f64>, SchemaError> {
    value
        .map(|value| {
            value
                .as_number()
                .ok_or_else(|| SchemaError::new(path, "must be a number"))
                .and_then(|value| finite_number(value, path))
        })
        .transpose()
}

fn finite_number(value: &Number, path: &SchemaPath) -> Result<f64, SchemaError> {
    value
        .as_f64()
        .filter(|value| value.is_finite())
        .ok_or_else(|| SchemaError::new(path, "must be a finite IEEE-754 number"))
}

fn string_array(value: &Value, path: &SchemaPath) -> Result<Value, SchemaError> {
    let values = value
        .as_array()
        .ok_or_else(|| SchemaError::new(path, "must be an array of strings"))?;
    let mut translated = Vec::with_capacity(values.len());
    for (index, value) in values.iter().enumerate() {
        let value = value
            .as_str()
            .ok_or_else(|| SchemaError::new(&path.child(index.to_string()), "must be a string"))?;
        translated.push(Value::String(value.to_string()));
    }
    Ok(Value::Array(translated))
}

fn is_true_schema(value: &Value) -> bool {
    value == &Value::Bool(true) || value.as_object().is_some_and(Map::is_empty)
}

fn merge_u64_bound(output: &mut Map<String, Value>, field: &str, value: u64, lower: bool) {
    let merged = output
        .get(field)
        .and_then(Value::as_u64)
        .map_or(value, |current| {
            if lower {
                current.max(value)
            } else {
                current.min(value)
            }
        });
    output.insert(field.to_string(), Value::Number(merged.into()));
}

fn next_up(value: f64) -> Option<f64> {
    if !value.is_finite() || value == f64::MAX {
        return None;
    }
    if value == 0.0 {
        return Some(f64::from_bits(1));
    }
    let bits = value.to_bits();
    Some(f64::from_bits(if value > 0.0 {
        bits + 1
    } else {
        bits - 1
    }))
}

fn next_down(value: f64) -> Option<f64> {
    if !value.is_finite() || value == -f64::MAX {
        return None;
    }
    if value == 0.0 {
        return Some(-f64::from_bits(1));
    }
    let bits = value.to_bits();
    Some(f64::from_bits(if value > 0.0 {
        bits - 1
    } else {
        bits + 1
    }))
}

fn pointer_path(root: &SchemaPath, pointer: &str) -> Result<SchemaPath, SchemaError> {
    let mut path = root.clone();
    for segment in pointer.strip_prefix('/').unwrap_or(pointer).split('/') {
        let segment = decode_pointer_segment(segment).ok_or_else(|| {
            SchemaError::new(root, "reference contains an invalid JSON Pointer escape")
        })?;
        path.segments.push(segment);
    }
    Ok(path)
}

fn decode_pointer_segment(segment: &str) -> Option<String> {
    let mut decoded = String::with_capacity(segment.len());
    let mut chars = segment.chars();
    while let Some(character) = chars.next() {
        if character != '~' {
            decoded.push(character);
            continue;
        }
        match chars.next()? {
            '0' => decoded.push('~'),
            '1' => decoded.push('/'),
            _ => return None,
        }
    }
    Some(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn official_google_vocabulary_is_preserved_and_nested() {
        let schema = json!({
            "type": "object", "title": "Root", "description": "test",
            "properties": {
                "s": {"type":"string", "format":"date-time", "minLength":1,
                    "maxLength":64, "pattern":"^[0-9]", "default":"x", "example":"y"},
                "n": {"type":"number", "minimum":0, "maximum":10},
                "a": {"type":"array", "items":{"type":"integer"}, "minItems":1,
                    "maxItems":3},
                "o": {"type":"object", "properties":{"x":{"type":"boolean"}},
                    "required":["x"], "minProperties":1, "maxProperties":1,
                    "propertyOrdering":["x"]},
                "u": {"anyOf":[{"type":"string"},{"type":"number"}]},
                "z": {"type":"string", "nullable":true}
            },
            "required": ["s"], "propertyOrdering": ["s","n","a","o","u","z"]
        });
        let translated = translate(&schema, "tools.0.parameters").unwrap();
        assert_eq!(translated["properties"]["n"]["minimum"].as_f64(), Some(0.0));
        assert_eq!(
            translated["properties"]["n"]["maximum"].as_f64(),
            Some(10.0)
        );
        let mut expected = schema;
        expected["properties"]["n"]["minimum"] = json!(0.0);
        expected["properties"]["n"]["maximum"] = json!(10.0);
        assert_eq!(translated, expected);
    }

    #[test]
    fn refs_const_nullable_and_exclusive_bounds_translate_exactly() {
        let schema = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$defs": {
                "Mode": {"type":"string", "const":"fast"},
                "Timeout": {"type":"number", "exclusiveMinimum":0, "exclusiveMaximum":10}
            },
            "type":"object",
            "properties": {
                "$schema": {"$ref":"#/$defs/Mode"},
                "timeout": {"$ref":"#/$defs/Timeout"},
                "note": {"type":["string","null"]}
            }
        });
        let translated = translate(&schema, "tools.0.parameters").unwrap();
        assert_eq!(
            translated["properties"]["$schema"],
            json!({
                "type":"string", "enum":["fast"]
            })
        );
        assert_eq!(
            translated["properties"]["note"],
            json!({
                "type":"string", "nullable":true
            })
        );
        assert_eq!(
            translated["properties"]["timeout"]["minimum"].as_f64(),
            Some(f64::from_bits(1))
        );
        assert!(
            translated["properties"]["timeout"]["maximum"]
                .as_f64()
                .unwrap()
                < 10.0
        );
        assert!(translated.get("$defs").is_none());
    }

    #[test]
    fn const_bounds_and_nullable_enum_intersections_never_widen() {
        assert_eq!(
            translate(
                &json!({"type":"number", "const":5, "minimum":4, "maximum":6}),
                "schema",
            )
            .unwrap(),
            json!({"type":"integer", "minimum":5.0, "maximum":5.0})
        );
        let error =
            translate(&json!({"type":"number", "const":5, "minimum":6}), "schema").unwrap_err();
        assert_eq!(error.path(), "schema/minimum");

        assert_eq!(
            translate(&json!({"type":["string", "null"], "const":null}), "schema",).unwrap(),
            json!({"type":"null"})
        );
        assert_eq!(
            translate(&json!({"type":"string", "enum":["x", null]}), "schema",).unwrap(),
            json!({"type":"string", "enum":["x"]})
        );
        assert_eq!(
            translate(
                &json!({"type":["string", "null"], "enum":["x", null]}),
                "schema",
            )
            .unwrap(),
            json!({"type":"string", "enum":["x"], "nullable":true})
        );
    }

    #[test]
    fn malformed_and_empty_intersections_fail_locally() {
        for (schema, suffix) in [
            (json!({"type":"string", "const":"x", "enum":"x"}), "/enum"),
            (
                json!({"type":"number", "minimum":2, "maximum":1}),
                "/maximum",
            ),
            (
                json!({"type":"array", "minItems":2, "maxItems":1}),
                "/maxItems",
            ),
            (
                json!({"type":"array", "contains":{}, "minContains":2, "maxContains":1}),
                "/maxContains",
            ),
        ] {
            let error = translate(&schema, "schema").unwrap_err();
            assert_eq!(error.path(), format!("schema{suffix}"), "{schema}");
        }
    }

    #[test]
    fn harmless_annotations_and_semantic_no_ops_are_removed() {
        let schema = json!({
            "$id":"urn:test", "$comment":"ignored", "examples":[1], "x-ui":"ignored",
            "type":"object", "additionalProperties":true, "unevaluatedProperties":{},
            "patternProperties":{}, "dependentRequired":{"x":[]}, "propertyNames":{},
            "properties":{"x":{"type":"string", "exclusiveMinimum":1}}
        });
        assert_eq!(
            translate(&schema, "schema").unwrap(),
            json!({
                "type":"object", "properties":{"x":{"type":"string"}}
            })
        );
    }

    #[test]
    fn audited_and_other_unrepresentable_constraints_fail_at_exact_pointer() {
        let cases = [
            (
                json!({"type":"object","patternProperties":{"^x":{"type":"string"}}}),
                "/patternProperties",
            ),
            (
                json!({"type":"object","dependentRequired":{"x":["y"]}}),
                "/dependentRequired",
            ),
            (
                json!({"type":"object","unevaluatedProperties":false}),
                "/unevaluatedProperties",
            ),
            (
                json!({"type":"object","propertyNames":{"pattern":"^x","type":"string"}}),
                "/propertyNames",
            ),
            (
                json!({"type":"object","if":{"properties":{}},"then":{"required":["x"]}}),
                "/if",
            ),
            (
                json!({"type":"array","contains":{"type":"string"},"minContains":2}),
                "/contains",
            ),
            (
                json!({"type":"object","additionalProperties":false}),
                "/additionalProperties",
            ),
            (json!({"type":"integer","multipleOf":2}), "/multipleOf"),
            (json!({"type":"array","uniqueItems":true}), "/uniqueItems"),
        ];
        for (schema, suffix) in cases {
            let error = translate(&schema, "tools.0.parameters").unwrap_err();
            assert_eq!(
                error.path(),
                format!("tools.0.parameters{suffix}"),
                "{schema}"
            );
        }
    }

    #[test]
    fn missing_external_and_recursive_references_fail_closed() {
        for (schema, suffix) in [
            (json!({"$ref":"https://example.com/schema"}), "/$ref"),
            (json!({"$ref":"#/$defs/Missing","$defs":{}}), "/$ref"),
            (
                json!({"$ref":"#/$defs/Loop","$defs":{"Loop":{"$ref":"#/$defs/Loop"}}}),
                "/$defs/Loop/$ref",
            ),
        ] {
            let error = translate(&schema, "schema").unwrap_err();
            assert_eq!(error.path(), format!("schema{suffix}"));
        }
    }
}
