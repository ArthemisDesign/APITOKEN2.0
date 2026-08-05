use super::*;
use crate::pricing::{PricingRuntimeCapabilityEvidence, PRICING_SCHEMA_VERSION};

fn manifest() -> PricingRuntimeManifestEvidence {
    PricingRuntimeManifestEvidence::new(
        1,
        vec![
            PricingRuntimeCapabilityEvidence::new(PRICING_SCHEMA_VERSION, 1, "capability").unwrap(),
        ],
    )
    .unwrap()
}

#[test]
fn request_bounds_are_fail_closed() {
    let valid = Stage8EngineEvidenceRequest {
        target_generation: 1,
        recovery_generation: 2,
        window_start_ts: 100,
        window_end_ts: 200,
        min_samples_per_provider: 1,
        financial_sample_size: 1,
        gemini_client_admissions: 0,
        runtime_manifest: manifest(),
    };
    assert!(valid.validate(200).is_ok());
    let mut invalid = valid.clone();
    invalid.window_end_ts = 99;
    assert!(invalid.validate(200).is_err());
    invalid = valid.clone();
    invalid.min_samples_per_provider = 0;
    assert!(invalid.validate(200).is_err());
    invalid = valid.clone();
    invalid.financial_sample_size = 1_001;
    assert!(invalid.validate(200).is_err());
    invalid = valid;
    invalid.gemini_client_admissions = -1;
    assert!(invalid.validate(200).is_err());
}

#[test]
fn provider_coverage_requires_google_and_accepts_all_three_authorities() {
    let mut counts = BTreeMap::from([("anthropic".to_owned(), 100), ("openai".to_owned(), 100)]);
    assert_eq!(
        insufficient_provider_coverage(&counts, 100),
        vec!["google:0"]
    );
    counts.insert("google".to_owned(), 100);
    assert!(insufficient_provider_coverage(&counts, 100).is_empty());
}

#[test]
fn release_inventory_and_funding_digests_match_the_typescript_canonical_contract() {
    let inventory = vec![
        EngineInventoryIdentity {
            account_id: "a",
            multiplier_bp: 5_000,
            status: "active",
        },
        EngineInventoryIdentity {
            account_id: "b",
            multiplier_bp: 10_000,
            status: "disabled",
        },
    ];
    assert_eq!(
        sha256_v2_json(b"pricing-stage5-v2:engine-identity-inventory\n", &inventory,).unwrap(),
        "sha256:v2:a8ed9afc4feeaf0e4f648ad55533ea87dd99852f796ee035713636b0e99258b0"
    );

    let funding = CanonicalScope {
        scope: "pricing-funding-normalization-manifest-v2",
        value: vec![FundingManifestIdentity {
            account_id: "a",
            funding_digest:
                "sha256:v2:1111111111111111111111111111111111111111111111111111111111111111",
            funding_generation: "1".to_owned(),
        }],
    };
    assert_eq!(
        sha256_v2_json(b"", &funding).unwrap(),
        "sha256:v2:96886f1dc94223e2ef37fbbc1bad307ad1cf84ec22f21b586253072f48307fef"
    );
}
