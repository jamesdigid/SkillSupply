use std::str::FromStr;

use serde_json::json;
use skillsupport::contract::{
    Contract, ContractSha, ContractStore, ContractStoreError, ExecutionMode,
    FilesystemContractStore, canonical_bytes,
};

fn contract(name: &str) -> Contract {
    Contract {
        name: name.to_string(),
        version: Some("1.0.0".to_string()),
        summary: Some(format!("Run {name}")),
        params: Some(json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" },
                "timeout": { "type": "number" }
            }
        })),
        result: Some(json!({ "type": "object" })),
        execution: ExecutionMode::Immediate,
        cancelable: false,
        refs: Vec::new(),
    }
}

#[test]
fn canonical_form_is_stable_across_object_key_order() {
    let left = json!({
        "b": 2,
        "a": {
            "d": 4,
            "c": 3
        }
    });
    let right = json!({
        "a": {
            "c": 3,
            "d": 4
        },
        "b": 2
    });

    assert_eq!(
        canonical_bytes(&left).expect("left canonical bytes"),
        canonical_bytes(&right).expect("right canonical bytes")
    );
}

#[test]
fn store_computes_deterministic_sha_and_round_trips() {
    let tempdir = tempfile::tempdir().expect("temp dir");
    let store = FilesystemContractStore::new(tempdir.path());
    let contract = contract("browser.navigate");

    let first = store.insert(&contract).expect("insert contract");
    let second = store.insert(&contract).expect("insert contract again");

    assert_eq!(first, second);
    assert_eq!(store.get(&first).expect("load contract"), Some(contract));
}

#[test]
fn malformed_sha_is_rejected() {
    assert!(ContractSha::from_str("sha256:not-hex").is_err());
    assert!(ContractSha::from_str("browser.navigate").is_err());
}

#[test]
fn dangling_ref_is_rejected() {
    let tempdir = tempfile::tempdir().expect("temp dir");
    let store = FilesystemContractStore::new(tempdir.path());
    let missing = ContractSha::from_str(
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .expect("valid sha");
    let mut contract = contract("browser.screenshot");
    contract.refs.push(missing.clone());

    let error = store.insert(&contract).expect_err("dangling ref");

    assert!(matches!(
        error,
        ContractStoreError::DanglingRef { sha } if sha == missing
    ));
}
