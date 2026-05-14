// Copyright (c) Jonathan Shook
// SPDX-License-Identifier: Apache-2.0

//! Schema metadata for the `--describe-output-json` document: the
//! version constant, the URN identifier, and the embedded JSON
//! Schema (draft 2020-12) that downstream tools can validate against.

/// Output-documentation structure version. Bumped on breaking
/// changes to the document shape.
pub const OUTPUT_DOC_VERSION: &str = "1";

/// Stable identifier for the JSON Schema describing
/// `--describe-output-json` output.
pub const OUTPUT_DOC_SCHEMA_URN: &str = "urn:cargo-summary:output-doc-schema:v1";

/// JSON Schema (draft 2020-12) for the `--describe-output-json`
/// document. Embedded as a string so the schema travels with the
/// binary.
pub const OUTPUT_DOC_SCHEMA: &str = r##"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "urn:cargo-summary:output-doc-schema:v1",
  "title": "cargo-summary output description",
  "description": "Schema for the JSON document emitted by `cargo-summary --describe-output-json`. Describes the human-readable summary lines that cargo-summary emits for build/check/test/clippy invocations.",
  "type": "object",
  "required": ["schema", "doc_version", "wrapper_version", "subcommands", "log_capture", "lines"],
  "additionalProperties": false,
  "properties": {
    "schema": {
      "type": "string",
      "description": "URN identifying the schema this document conforms to. Fetch via `cargo-summary --describe-output-schema`."
    },
    "doc_version": {
      "type": "string",
      "description": "Version of the output-description structure; bumped on breaking changes."
    },
    "wrapper_version": {
      "type": "string",
      "description": "Version of the cargo-summary binary that produced this document."
    },
    "subcommands": {
      "type": "array",
      "description": "Cargo subcommands cargo-summary parses and summarizes. All others are forwarded raw unless --wrap-unknown is set.",
      "items": { "$ref": "#/$defs/subcommand" }
    },
    "log_capture": {
      "type": "object",
      "required": ["description", "suffix_grammar", "file_layout"],
      "additionalProperties": false,
      "properties": {
        "description":    { "type": "string" },
        "suffix_grammar": { "type": "string" },
        "file_layout":    { "type": "string" }
      }
    },
    "lines": {
      "type": "array",
      "description": "One entry per summary line shape cargo-summary may emit.",
      "items": { "$ref": "#/$defs/line" }
    }
  },
  "$defs": {
    "subcommand": {
      "type": "object",
      "required": ["name", "cargo_invocation", "summary_kinds", "notes"],
      "additionalProperties": false,
      "properties": {
        "name":             { "type": "string", "description": "Cargo subcommand cargo-summary recognizes." },
        "cargo_invocation": { "type": "string", "description": "Exact cargo invocation used. May list alternatives separated by ' | '." },
        "summary_kinds":    { "type": "array", "items": { "type": "string" }, "description": "Kind identifiers (see lines[].kind) this subcommand may produce." },
        "notes":            { "type": "string" }
      }
    },
    "line": {
      "type": "object",
      "required": ["kind", "label", "grammar", "notes", "fields", "examples"],
      "additionalProperties": false,
      "properties": {
        "kind":     { "type": "string", "description": "Stable snake_case identifier for this line shape." },
        "label":    { "type": "string", "description": "Human-readable label." },
        "grammar":  { "type": "string", "description": "Production-rule-style grammar." },
        "notes":    { "type": "string", "description": "Free-form notes; may be empty." },
        "fields":   { "type": "array", "items": { "$ref": "#/$defs/field" } },
        "examples": { "type": "array", "items": { "type": "string", "description": "Concrete example, produced by the live renderer." } }
      }
    },
    "field": {
      "type": "object",
      "required": ["name", "type", "optional", "description"],
      "additionalProperties": false,
      "properties": {
        "name":        { "type": "string" },
        "type":        { "type": "string", "description": "Informal type label (u64, f64, enum, list<string>, ...)." },
        "optional":    { "type": "boolean" },
        "description": { "type": "string" }
      }
    }
  }
}
"##;
