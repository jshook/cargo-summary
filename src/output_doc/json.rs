// Copyright (c) Jonathan Shook
// SPDX-License-Identifier: Apache-2.0

//! JSON rendering of the output documentation (the content of
//! `cargo summary --describe-output-json`).

use super::output_doc;

/// Render the output documentation as pretty-printed JSON.
///
/// # Panics
///
/// Panics if the embedded [`crate::OutputDoc`] cannot be serialized.
/// This is structurally impossible -- every field is a known type
/// with a working `Serialize` impl over static data -- so the panic
/// exists only to surface a programmer error if the schema ever
/// drifts from what `serde` can handle.
#[must_use]
pub fn output_doc_json(wrapper_version: &'static str) -> String {
    serde_json::to_string_pretty(&output_doc(wrapper_version)).expect("output doc is serializable")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OUTPUT_DOC_SCHEMA_URN, SubcommandKind};

    #[test]
    fn carries_schema_urn_and_subcommands() {
        let raw = output_doc_json("0.0.0-test");
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            v.get("schema").and_then(|s| s.as_str()),
            Some(OUTPUT_DOC_SCHEMA_URN),
        );
        assert_eq!(
            v.get("wrapper_version").and_then(|s| s.as_str()),
            Some("0.0.0-test")
        );
        let subs = v
            .get("subcommands")
            .and_then(|s| s.as_array())
            .expect("subcommands");
        let names: Vec<&str> = subs
            .iter()
            .map(|s| s.get("name").and_then(|n| n.as_str()).unwrap_or(""))
            .collect();
        for n in SubcommandKind::ALL_NAMES {
            assert!(names.contains(n), "subcommand {n} missing from doc");
        }
    }
}
