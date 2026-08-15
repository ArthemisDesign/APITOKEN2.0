use super::*;
use axum::body::{to_bytes, Body};
use tower::ServiceExt;

fn unknown_codex_status() -> forward::codex::CodexOperationalStatus {
    forward::codex::CodexOperationalStatus {
        process_live: true,
        rate_limits: None,
        homes: vec![forward::codex::CodexHomeStatus {
            id: "home-1".to_string(),
            masked_email: "owne…".to_string(),
            plan: "chatgpt_pro".to_string(),
            acquired_at: Some(105),
            subscription_expires_at: Some(2_592_105),
            subscription_days_left: Some(30.0),
            process_live: true,
            auth_ok: true,
            account_state: "healthy",
            transport_state: "responsive",
            admitted: true,
            ready_published: true,
            reject_reason: None,
            snapshot_age_secs: Some(5),
            cooling_until: 0,
            inflight: 0,
            rate_limits: None,
            limit_reached: false,
            spend_nano_total: 12_500_000_000,
            spend_usd_total: 12.5,
            spend_nanocredits_total: Some(1_250_000_000),
            credit_tracking_started_ts: Some(90),
            calibration_pending_events: 0,
            calibration_dropped_events: 0,
            calibration_persistence_ok: true,
            capacities: vec![forward::codex::CodexWindowCapacityReport {
                slot: "primary",
                window_minutes: Some(300),
                resets_at: Some(2_000_000_000),
                observed_at: 100,
                data_age_seconds: Some(5),
                used_fraction_units: 40_000_000,
                used_percent: 40,
                measurement_resolution_fraction_units: 1_000_000,
                capacity_nano: None,
                remaining_nano: None,
                low_nano: None,
                high_nano: None,
                remaining_low_nano: None,
                remaining_high_nano: None,
                cap_usd: None,
                remaining_usd: None,
                low_usd: None,
                high_usd: None,
                remaining_low_usd: None,
                remaining_high_usd: None,
                capacity_nanocredits: None,
                remaining_nanocredits: None,
                low_nanocredits: None,
                high_nanocredits: None,
                remaining_low_nanocredits: None,
                remaining_high_nanocredits: None,
                observed_spend_nanocredits: Some(0),
                credit_samples: Some(0),
                unattributed_fraction_units: Some(0),
                observed_spend_nano: 0,
                observed_fraction_units: 0,
                source: "unknown",
                confidence: 0.0,
                samples: 0,
            }],
            fast_tiers: vec![forward::codex::CodexFastTierStatus {
                model: "gpt-5.6-sol".to_string(),
                catalog_available: Some(true),
                catalog_fast_supported: Some(true),
                served_tier: Some("priority"),
                provider_reported_tier: Some("default"),
                observed_at: Some(101),
            }],
        }],
        available: 1,
        soonest_ready: None,
    }
}

fn unknown_gemini_status() -> forward::GeminiOperationalStatus {
    let window = |bucket_id, window_kind, window_minutes, remaining_fraction_units| {
        forward::GeminiWindowCapacityReport {
            bucket_id,
            window_kind,
            window_minutes,
            resets_at: 2_000_000_000,
            observed_at: 100,
            data_age_seconds: 5,
            remaining_fraction_units,
            used_fraction_units: 100_000_000 - remaining_fraction_units,
            capacity_nano: None,
            remaining_nano: None,
            low_nano: None,
            high_nano: None,
            remaining_low_nano: None,
            remaining_high_nano: None,
            cap_usd: None,
            remaining_usd: None,
            low_usd: None,
            high_usd: None,
            remaining_low_usd: None,
            remaining_high_usd: None,
            observed_spend_nano: 0,
            observed_fraction_units: 0,
            source: "unknown",
            confidence: 0.0,
            samples: 0,
        }
    };
    forward::GeminiOperationalStatus {
        profiles: vec![forward::GeminiProfileStatus {
            id: "profile-opaque".to_string(),
            masked_email: "owne…".to_string(),
            plan: "google_ai_pro".to_string(),
            acquired_at: Some(105),
            subscription_expires_at: Some(47_174_505),
            subscription_days_left: Some(546.0),
            authenticated: true,
            disabled: false,
            hidden: false,
            cooling_until: 0,
            inflight: 0,
            last_probe_at: 100,
            quota_updated_at: 100,
            quotas: Vec::new(),
            model_cooling: Vec::new(),
            spend_usd_total: 0.019404,
            calibration_persistence_ok: true,
            capacities: vec![
                window("gemini-5h", "5h", 300, 75_000_000),
                window("gemini-weekly", "weekly", 10_080, 60_000_000),
            ],
        }],
        models: Vec::new(),
        credential_generation_digest: format!("blake3:{}", "0".repeat(64)),
        available: 1,
        authenticated: 1,
        soonest_ready: None,
    }
}

#[test]
fn codex_subscription_contract_publishes_the_admission_verdict() {
    let mut status = unknown_codex_status();
    let value = codex_subs_value(&status, 105);
    assert_eq!(value["homes"][0]["email"], "owne…");
    assert_eq!(value["homes"][0]["plan"], "chatgpt_pro");
    assert_eq!(value["homes"][0]["acquired_at"], 105);
    assert_eq!(value["homes"][0]["subscription_expires_at"], 2_592_105);
    assert_eq!(value["homes"][0]["subscription_days_left"], 30.0);
    assert_eq!(value["homes"][0]["limit_reached"], false);
    assert_eq!(value["homes"][0]["spend_nanocredits_total"], "1250000000");
    assert_eq!(value["homes"][0]["credit_tracking_started_ts"], 90);
    assert_eq!(value["homes"][0]["calibration_pending_events"], 0);
    assert_eq!(value["homes"][0]["calibration_dropped_events"], 0);
    assert_eq!(value["calibration_evidence_available"], false);

    // A home the gateway refuses to route to must never read as active on an operator surface.
    status.homes[0].limit_reached = true;
    status.homes[0].capacities[0].used_percent = 100;
    status.homes[0].capacities[0].used_fraction_units = 100_000_000;
    status.available = 0;
    let value = codex_subs_value(&status, 105);
    assert_eq!(value["homes"][0]["limit_reached"], true);
    assert_eq!(value["available"], 0);
    assert_eq!(
        value["homes"][0]["windows"][0]["used_percent"], 100,
        "the exhausted window stays visible next to the verdict"
    );
}

#[test]
fn codex_subscription_contract_separates_effective_fast_from_provider_report() {
    let value = codex_subs_value(&unknown_codex_status(), 105);
    let tier = &value["homes"][0]["fast_tiers"][0];
    assert_eq!(tier["model"], "gpt-5.6-sol");
    assert_eq!(tier["catalog_fast_supported"], true);
    assert_eq!(tier["served_tier"], "priority");
    assert_eq!(tier["provider_reported_tier"], "default");
    assert_eq!(tier["observed_at"], 101);
}

#[test]
fn codex_subscription_contract_keeps_unmeasured_capacity_null() {
    let mut status = unknown_codex_status();
    let mut duplicate_slot = status.homes[0].capacities[0].clone();
    duplicate_slot.slot = "secondary";
    status.homes[0].capacities.push(duplicate_slot);
    let value = codex_subs_value(&status, 105);
    let window = &value["homes"][0]["windows"][0];
    assert_eq!(window["window_minutes"], 300);
    assert_eq!(window["source"], "unknown");
    assert_eq!(window["used_fraction_units"], 40_000_000);
    assert_eq!(window["used_fraction"], 0.4);
    assert_eq!(window["measurement_resolution_fraction_units"], 1_000_000);
    assert_eq!(window["workload_dependent"], true);
    assert!(window["capacity_nano"].is_null());
    assert!(window["cap_usd"].is_null());
    assert!(window["remaining_usd"].is_null());

    let total = &value["window_totals"][0];
    assert_eq!(total["window_minutes"], 300);
    assert_eq!(
        total["observed_homes"], 1,
        "one home must not be counted twice"
    );
    assert_eq!(total["measured_homes"], 0);
    assert!(total["cap_usd"].is_null());
    assert!(total["remaining_usd"].is_null());
    let cohort = &value["plan_cohorts"][0];
    assert_eq!(cohort["plan"], "chatgpt_pro");
    assert_eq!(cohort["homes_total"], 1);
    assert_eq!(cohort["measured_homes"], 0);
    assert!(cohort["capacity_per_home_nanocredits"].is_null());
    assert_eq!(cohort["source"], "unknown");
}

#[test]
fn codex_plan_cohort_pools_equal_plans_without_overwriting_home_evidence() {
    let mut status = unknown_codex_status();
    let first = &mut status.homes[0].capacities[0];
    first.capacity_nanocredits = Some(45_000_000_000_000);
    first.remaining_nanocredits = Some(27_000_000_000_000);
    first.low_nanocredits = Some(30_000_000_000_000);
    first.high_nanocredits = Some(90_000_000_000_000);
    first.remaining_low_nanocredits = Some(18_000_000_000_000);
    first.remaining_high_nanocredits = Some(54_000_000_000_000);
    first.observed_spend_nanocredits = Some(900_000_000_000);
    first.observed_fraction_units = 2_000_000;
    first.credit_samples = Some(1);

    let mut second_home = status.homes[0].clone();
    second_home.id = "home-2".into();
    second_home.masked_email = "seco…".into();
    let second = &mut second_home.capacities[0];
    second.used_fraction_units = 14_000_000;
    second.used_percent = 14;
    second.capacity_nanocredits = Some(60_000_000_000_000);
    second.remaining_nanocredits = Some(51_600_000_000_000);
    second.low_nanocredits = Some(40_000_000_000_000);
    second.high_nanocredits = Some(120_000_000_000_000);
    second.remaining_low_nanocredits = Some(34_400_000_000_000);
    second.remaining_high_nanocredits = Some(103_200_000_000_000);
    second.observed_spend_nanocredits = Some(1_800_000_000_000);
    second.observed_fraction_units = 3_000_000;
    second.credit_samples = Some(1);

    let mut unmeasured_home = status.homes[0].clone();
    unmeasured_home.id = "home-3".into();
    unmeasured_home.masked_email = "thir…".into();
    unmeasured_home.capacities[0].used_fraction_units = 5_000_000;
    unmeasured_home.capacities[0].used_percent = 5;
    unmeasured_home.capacities[0].capacity_nanocredits = None;
    unmeasured_home.capacities[0].remaining_nanocredits = None;
    unmeasured_home.capacities[0].low_nanocredits = None;
    unmeasured_home.capacities[0].high_nanocredits = None;
    unmeasured_home.capacities[0].remaining_low_nanocredits = None;
    unmeasured_home.capacities[0].remaining_high_nanocredits = None;
    unmeasured_home.capacities[0].observed_spend_nanocredits = Some(0);
    unmeasured_home.capacities[0].observed_fraction_units = 0;
    unmeasured_home.capacities[0].credit_samples = Some(0);
    let mut duplicate_slot = unmeasured_home.capacities[0].clone();
    duplicate_slot.slot = "secondary";
    unmeasured_home.capacities.push(duplicate_slot);

    let mut other_plan = status.homes[0].clone();
    other_plan.id = "home-plus".into();
    other_plan.plan = "chatgpt_plus".into();
    let plus = &mut other_plan.capacities[0];
    plus.capacity_nanocredits = Some(10_000_000_000_000);
    plus.remaining_nanocredits = Some(6_000_000_000_000);
    plus.low_nanocredits = Some(8_000_000_000_000);
    plus.high_nanocredits = Some(12_000_000_000_000);
    plus.remaining_low_nanocredits = Some(4_800_000_000_000);
    plus.remaining_high_nanocredits = Some(7_200_000_000_000);
    plus.observed_spend_nanocredits = Some(100_000_000_000);
    plus.observed_fraction_units = 1_000_000;
    plus.credit_samples = Some(1);

    status
        .homes
        .extend([second_home, unmeasured_home, other_plan]);
    let value = codex_subs_value(&status, 105);
    let cohorts = value["plan_cohorts"].as_array().unwrap();
    let pro = cohorts
        .iter()
        .find(|cohort| cohort["plan"] == "chatgpt_pro")
        .unwrap();
    assert_eq!(pro["window_minutes"], 300);
    assert_eq!(pro["homes_total"], 3);
    assert_eq!(pro["measured_homes"], 2);
    assert_eq!(pro["observed_fraction_units"], "5000000");
    assert_eq!(pro["observed_spend_nanocredits"], "2700000000000");
    assert_eq!(pro["capacity_per_home_nanocredits"], "54000000000000");
    assert_eq!(pro["capacity_per_home_low_nanocredits"], "30000000000000");
    assert_eq!(pro["capacity_per_home_high_nanocredits"], "120000000000000");
    assert_eq!(pro["fleet_capacity_nanocredits"], "162000000000000");
    assert_eq!(pro["fleet_remaining_nanocredits"], "130140000000000");
    assert_eq!(pro["source"], "plan_pooled_native_credits");
    assert_eq!(pro["same_plan_capacity"], true);
    assert_eq!(pro["workload_dependent"], false);

    assert_eq!(
        value["homes"][0]["windows"][0]["capacity_nanocredits"], "45000000000000",
        "the pooled plan answer must not overwrite immutable per-home evidence"
    );
    assert_eq!(
        value["homes"][1]["windows"][0]["capacity_nanocredits"],
        "60000000000000"
    );
    let plus = cohorts
        .iter()
        .find(|cohort| cohort["plan"] == "chatgpt_plus")
        .unwrap();
    assert_eq!(plus["homes_total"], 1);
    assert_eq!(plus["capacity_per_home_nanocredits"], "10000000000000");
}

#[test]
fn codex_plan_cohort_keeps_upper_bound_unknown_if_any_evidence_is_one_sided() {
    let mut status = unknown_codex_status();
    let first = &mut status.homes[0].capacities[0];
    first.capacity_nanocredits = Some(45_000_000_000_000);
    first.remaining_nanocredits = Some(27_000_000_000_000);
    first.low_nanocredits = Some(30_000_000_000_000);
    first.high_nanocredits = None;
    first.observed_spend_nanocredits = Some(900_000_000_000);
    first.observed_fraction_units = 1_000_000;
    first.credit_samples = Some(1);

    let value = codex_subs_value(&status, 105);
    let cohort = &value["plan_cohorts"][0];
    assert_eq!(cohort["capacity_per_home_nanocredits"], "90000000000000");
    assert_eq!(
        cohort["capacity_per_home_low_nanocredits"],
        "30000000000000"
    );
    assert!(cohort["capacity_per_home_high_nanocredits"].is_null());
    assert!(cohort["fleet_remaining_high_nanocredits"].is_null());
}

#[test]
fn codex_subscription_contract_publishes_exact_workload_capacity_and_remaining() {
    let mut status = unknown_codex_status();
    let capacity = &mut status.homes[0].capacities[0];
    capacity.capacity_nano = Some(2_450_041_880_000);
    capacity.remaining_nano = Some(1_470_025_128_000);
    capacity.low_nano = Some(2_449_980_630_000);
    capacity.high_nano = Some(2_450_103_133_000);
    capacity.remaining_low_nano = Some(1_469_988_378_000);
    capacity.remaining_high_nano = Some(1_470_061_880_000);
    capacity.cap_usd = Some(2_450.04188);
    capacity.remaining_usd = Some(1_470.025128);
    capacity.low_usd = Some(2_449.98063);
    capacity.high_usd = Some(2_450.103133);
    capacity.remaining_low_usd = Some(1_469.988378);
    capacity.remaining_high_usd = Some(1_470.06188);
    capacity.observed_spend_nano = 980_016_752_000;
    capacity.observed_fraction_units = 40_000_000;
    capacity.capacity_nanocredits = Some(2_000_000_000_000);
    capacity.remaining_nanocredits = Some(1_200_000_000_000);
    capacity.low_nanocredits = Some(1_900_000_000_000);
    capacity.high_nanocredits = Some(2_100_000_000_000);
    capacity.remaining_low_nanocredits = Some(1_140_000_000_000);
    capacity.remaining_high_nanocredits = Some(1_260_000_000_000);
    capacity.observed_spend_nanocredits = Some(800_000_000_000);
    capacity.credit_samples = Some(4);
    capacity.unattributed_fraction_units = Some(250_000);
    capacity.source = "workload_blend";
    capacity.confidence = 0.8333;
    capacity.samples = 10;

    let value = codex_subs_value(&status, 105);
    let window = &value["homes"][0]["windows"][0];
    assert_eq!(value["homes"][0]["spend_nano_total"], "12500000000");
    assert_eq!(window["capacity_nano"], "2450041880000");
    assert_eq!(window["remaining_nano"], "1470025128000");
    assert_eq!(window["remaining_low_nano"], "1469988378000");
    assert_eq!(window["remaining_high_nano"], "1470061880000");
    assert_eq!(window["observed_spend_nano"], "980016752000");
    assert_eq!(window["capacity_nanocredits"], "2000000000000");
    assert_eq!(window["remaining_nanocredits"], "1200000000000");
    assert_eq!(window["observed_spend_nanocredits"], "800000000000");
    assert_eq!(window["credit_samples"], 4);
    assert_eq!(window["unattributed_fraction_units"], 250_000);
    assert_eq!(window["observed_fraction_units"], 40_000_000);
    assert_eq!(window["workload_dependent"], true);
    assert_eq!(window["cap_usd"], 2_450.04);
    assert_eq!(window["remaining_usd"], 1_470.03);
    assert_eq!(window["source"], "workload_blend");
    assert_eq!(window["samples"], 10);
    assert_eq!(value["window_totals"][0]["capacity_nano"], "2450041880000");
    assert_eq!(
        value["window_totals"][0]["capacity_nanocredits"],
        "2000000000000"
    );
    assert_eq!(
        value["window_totals"][0]["remaining_nanocredits"],
        "1200000000000"
    );
    assert_eq!(
        value["window_totals"][0]["observed_spend_nanocredits"],
        "800000000000"
    );
    assert_eq!(value["window_totals"][0]["credit_measured_homes"], 1);
    assert_eq!(value["window_totals"][0]["credit_observed_homes"], 1);
    assert_eq!(
        value["window_totals"][0]["unattributed_fraction_units"],
        "250000"
    );
    assert_eq!(value["window_totals"][0]["source"], "workload_blend");
    assert_eq!(value["window_totals"][0]["measured_homes"], 1);

    let mut metrics = String::new();
    write_codex_home_capacity_metrics(&mut metrics, &status.homes[0]);
    assert!(metrics.contains(
        "claude_api_codex_home_window_used_ratio{home=\"home-1\",slot=\"primary\",window_minutes=\"300\"} 0.40000000"
    ));
    assert!(metrics.contains(
        "claude_api_codex_home_window_observed_spend_usd{home=\"home-1\",slot=\"primary\",window_minutes=\"300\"} 980.016752000"
    ));
    assert!(metrics.contains(
        "claude_api_codex_home_window_capacity_usd{home=\"home-1\",slot=\"primary\",window_minutes=\"300\",source=\"workload_blend\"} 2450.041880000"
    ));
    assert!(metrics.contains(
        "claude_api_codex_home_window_remaining_low_usd{home=\"home-1\",slot=\"primary\",window_minutes=\"300\"} 1469.988378000"
    ));
}

#[test]
fn codex_subscription_contract_publishes_immutable_turn_evidence() {
    let report = vec![registry::CodexTurnCalibrationAggregate {
        home_id: "home-1".into(),
        model_id: "gpt-5.6-sol".into(),
        service_tier: "fast".into(),
        provider_reported_tier: Some("priority".into()),
        api_tariff_schedule_id: "openai/gpt-5.6-sol/2026-07-30/v2".into(),
        credit_schedule_id: metering::CODEX_CREDIT_SCHEDULE_ID.into(),
        turns: 3,
        first_completed_at: 100,
        last_completed_at: 120,
        input_tokens: 1_000,
        cached_input_tokens: 400,
        cache_write_input_tokens: 100,
        output_tokens: 100,
        reasoning_output_tokens: 80,
        api_input_nanousd: 5_000_000,
        api_cached_input_nanousd: 400_000,
        api_cache_write_nanousd: 1_250_000,
        api_output_nanousd: 6_000_000,
        api_total_nanousd: 12_650_000,
        chatgpt_input_nanocredits: 187_500_000,
        chatgpt_cached_input_nanocredits: 12_500_000,
        chatgpt_output_nanocredits: 187_500_000,
        chatgpt_total_nanocredits: 387_500_000,
    }];
    let value = codex_subs_value_with_report(&unknown_codex_status(), 105, Some(&report));
    assert_eq!(value["calibration_evidence_available"], true);
    assert_eq!(
        value["credit_schedule_id"],
        metering::CODEX_CREDIT_SCHEDULE_ID
    );
    let evidence = &value["homes"][0]["calibration_evidence"][0];
    assert_eq!(evidence["model"], "gpt-5.6-sol");
    assert_eq!(evidence["service_tier"], "fast");
    assert_eq!(evidence["turns"], 3);
    assert_eq!(evidence["input_tokens"], "1000");
    assert_eq!(evidence["api_total_nanousd"], "12650000");
    assert_eq!(evidence["chatgpt_total_nanocredits"], "387500000");
}

#[test]
fn codex_conversion_catalogue_keeps_api_and_subscription_fast_independent() {
    let spec = metering::codex_catalog_at(1_785_369_601)
        .into_iter()
        .find(|model| model.id == "gpt-5.6-sol")
        .unwrap();
    let model = forward::codex::CodexModel {
        id: spec.id.into(),
        upstream: spec.upstream.into(),
        created: 0,
        owned_by: "test".into(),
        max_output_tokens: spec.max_output_tokens,
        reasoning_efforts: spec
            .reasoning_efforts
            .iter()
            .map(|value| (*value).into())
            .collect(),
        input_modalities: vec!["text".into(), "image".into()],
        output_modalities: vec!["text".into()],
        tool_calling: true,
        structured_outputs: true,
        fast_multiplier_basis_points: spec.subscription_fast_multiplier_basis_points,
        prices: spec.prices,
    };
    let values = codex_conversion_models(&[model], 1_785_369_601);
    assert_eq!(values.len(), 1);
    assert_eq!(
        values[0]["api_tariff_schedule_id"],
        "openai/gpt-5.6-sol/2026-07-30/v2"
    );
    assert_eq!(values[0]["api"]["input_nanousd_per_token"], "5000");
    assert_eq!(values[0]["api"]["fast_multiplier_basis_points"], 20_000);
    assert_eq!(
        values[0]["chatgpt_credits"]["input_nanocredits_per_token"],
        "125000"
    );
    assert_eq!(
        values[0]["chatgpt_credits"]["fast_multiplier_basis_points"],
        25_000
    );
}

#[test]
fn claude_conversion_catalogue_publishes_every_metered_token_bucket() {
    let values = anthropic_conversion_models(1_785_369_601);
    assert_eq!(values.len(), 7);
    let opus = values
        .iter()
        .find(|value| value["id"] == "claude-opus-4-8")
        .unwrap();
    assert_eq!(opus["tiers"][0]["id"], "standard");
    assert_eq!(opus["tiers"][0]["input_nanousd_per_token"], "5000");
    assert_eq!(
        opus["tiers"][0]["cache_write_1h_nanousd_per_token"],
        "10000"
    );
    assert_eq!(opus["tiers"][0]["output_nanousd_per_token"], "25000");
    assert_eq!(opus["tiers"][1]["id"], "fast");
    assert_eq!(opus["tiers"][1]["output_nanousd_per_token"], "50000");
    assert_eq!(opus["web_search_nanousd_per_request"], "10000000");

    let opus5 = values
        .iter()
        .find(|value| value["id"] == "claude-opus-5")
        .unwrap();
    assert_eq!(opus5["tiers"][1]["id"], "fast");
    let fable = values
        .iter()
        .find(|value| value["id"] == "claude-fable-5")
        .unwrap();
    assert_eq!(fable["tiers"].as_array().unwrap().len(), 1);
    assert_eq!(fable["tiers"][0]["input_nanousd_per_token"], "10000");
    assert_eq!(fable["tiers"][0]["output_nanousd_per_token"], "50000");
    let opus47 = values
        .iter()
        .find(|value| value["id"] == "claude-opus-4-7")
        .unwrap();
    assert_eq!(opus47["tiers"].as_array().unwrap().len(), 1);
}

#[test]
fn claude_email_hint_never_includes_the_domain_for_a_short_local_part() {
    assert_eq!(mask_claude_email("a@example.com"), "a…");
    assert_eq!(mask_claude_email("owner.account@example.com"), "owne…");
}

#[test]
fn claude_lifecycle_joins_full_identity_before_equal_masks_are_serialized() {
    let caps = vec![
        capacity("owner.one@example.com", 1.0, true, false),
        capacity("owner.two@example.com", 1.0, true, false),
    ];
    let lifecycle = BTreeMap::from([
        ("owner.one@example.com".to_string(), 100),
        ("owner.two@example.com".to_string(), 200),
    ]);
    let value = capacity_value_with_lifecycle(&caps, None, None, 300, Some(&lifecycle));

    assert_eq!(value["per_sub"][0]["email"], "owne…");
    assert_eq!(value["per_sub"][1]["email"], "owne…");
    assert_eq!(value["per_sub"][0]["acquired_at"], 100);
    assert_eq!(value["per_sub"][1]["acquired_at"], 200);
    assert_eq!(
        value["per_sub"][0]["subscription_expires_at"],
        100 + SUB_LIFETIME_DAYS * SECONDS_PER_DAY
    );
}

#[test]
fn claude_lifecycle_stays_null_without_valid_registry_authority() {
    let cap = capacity("owner@example.com", 1.0, true, false);
    let missing = capacity_value_with_lifecycle(&[cap.clone()], None, None, 300, None);
    assert!(missing["per_sub"][0]["acquired_at"].is_null());
    assert!(missing["per_sub"][0]["subscription_expires_at"].is_null());
    assert!(missing["per_sub"][0]["subscription_days_left"].is_null());

    let invalid = BTreeMap::from([("owner@example.com".to_string(), 0)]);
    let value = capacity_value_with_lifecycle(&[cap], None, None, 300, Some(&invalid));
    assert!(value["per_sub"][0]["acquired_at"].is_null());
}

#[test]
fn gemini_conversion_catalogue_keeps_long_context_media_and_quota_aliases() {
    let spec = metering::gemini_catalog_at(1_785_369_601)
        .into_iter()
        .find(|model| model.id == "gemini-3.1-pro-preview")
        .unwrap();
    let model = forward::GeminiModel {
        id: spec.id.into(),
        display_name: spec.display_name.into(),
        input_token_limit: spec.input_token_limit,
        output_token_limit: spec.output_token_limit,
        prices: spec.prices,
    };
    let values = gemini_conversion_models(&[model], 1_785_369_601);
    assert_eq!(values.len(), 1);
    assert_eq!(
        values[0]["tariff_schedule_id"],
        metering::gemini::TARIFF_SCHEDULE_ID
    );
    assert_eq!(values[0]["rates"]["input_nanousd_per_token"], "2000");
    assert_eq!(values[0]["rates"]["long_output_nanousd_per_token"], "18000");
    assert_eq!(values[0]["search"]["billing_unit"], "query");
    assert_eq!(
        values[0]["quota_model_ids"],
        json!(["gemini-3.1-pro-low", "gemini-pro-agent"])
    );

    let preview = metering::gemini_catalog_at(1_785_369_601)
        .into_iter()
        .find(|model| model.id == "gemini-3-flash-preview")
        .unwrap();
    let preview = forward::GeminiModel {
        id: preview.id.into(),
        display_name: preview.display_name.into(),
        input_token_limit: preview.input_token_limit,
        output_token_limit: preview.output_token_limit,
        prices: preview.prices,
    };
    let values = gemini_conversion_models(&[preview], 1_785_369_601);
    assert_eq!(values[0]["rates"]["input_nanousd_per_token"], "500");
    assert_eq!(values[0]["rates"]["audio_input_nanousd_per_token"], "1000");
    assert_eq!(values[0]["rates"]["output_nanousd_per_token"], "3000");
    assert_eq!(
        values[0]["quota_model_ids"],
        json!(["gemini-3-flash", "gemini-3-flash-agent"])
    );

    let flash_37 = metering::gemini_catalog_at(1_788_220_800)
        .into_iter()
        .find(|model| model.id == "gemini-3.7-flash")
        .unwrap();
    let flash_37 = forward::GeminiModel {
        id: flash_37.id.into(),
        display_name: flash_37.display_name.into(),
        input_token_limit: flash_37.input_token_limit,
        output_token_limit: flash_37.output_token_limit,
        prices: flash_37.prices,
    };
    let values = gemini_conversion_models(&[flash_37], 1_788_220_800);
    assert_eq!(values[0]["rates"]["input_nanousd_per_token"], "750");
    assert_eq!(values[0]["rates"]["cached_input_nanousd_per_token"], "75");
    assert_eq!(values[0]["rates"]["output_nanousd_per_token"], "3750");
    assert_eq!(
        values[0]["quota_model_ids"],
        json!(["gemini-3.7-flash-tiered"])
    );
}

#[test]
fn prometheus_omits_unmeasured_codex_dollar_series() {
    let home = &unknown_codex_status().homes[0];
    let mut body = String::new();
    write_codex_home_capacity_metrics(&mut body, home);
    assert!(body.contains(
        "claude_api_codex_home_window_estimate_available{home=\"home-1\",slot=\"primary\",window_minutes=\"300\",source=\"unknown\"} 0"
    ));
    assert!(body.contains(
        "claude_api_codex_home_window_data_age_seconds{home=\"home-1\",slot=\"primary\",window_minutes=\"300\"} 5"
    ));
    assert!(!body.contains("claude_api_codex_home_window_capacity_usd{"));
    assert!(!body.contains("claude_api_codex_home_window_remaining_usd{"));
}

#[test]
fn gemini_subscription_contract_keeps_five_hour_and_weekly_unknown_independently() {
    let mut status = unknown_gemini_status();
    let profiles = gemini_profile_values(&status, true, 105);
    assert_eq!(profiles[0]["email"], "owne…");
    assert_eq!(profiles[0]["plan"], "google_ai_pro");
    assert_eq!(profiles[0]["acquired_at"], 105);
    assert_eq!(profiles[0]["subscription_expires_at"], 47_174_505);
    assert_eq!(profiles[0]["subscription_days_left"], 546.0);
    assert_eq!(profiles[0]["windows"][0]["bucket_id"], "gemini-5h");
    assert_eq!(profiles[0]["windows"][1]["bucket_id"], "gemini-weekly");
    assert!(profiles[0]["windows"][0]["cap_usd"].is_null());
    assert!(profiles[0]["windows"][1]["cap_usd"].is_null());

    let totals = gemini_window_total_values(&status, true, 105);
    assert_eq!(totals[0]["window_minutes"], 300);
    assert_eq!(totals[0]["observed_profiles"], 1);
    assert_eq!(totals[0]["measured_profiles"], 0);
    assert!(totals[0]["cap_usd"].is_null());
    assert_eq!(totals[1]["window_minutes"], 10_080);
    assert_eq!(totals[1]["measured_profiles"], 0);

    let five_hour = &mut status.profiles[0].capacities[0];
    five_hour.cap_usd = Some(36.515628714);
    five_hour.remaining_usd = Some(27.386721535);
    five_hour.low_usd = Some(20.000244158);
    five_hour.high_usd = Some(81.204166575);
    five_hour.remaining_low_usd = Some(15.000183118);
    five_hour.remaining_high_usd = Some(60.903124931);
    five_hour.capacity_nano = Some(36_515_628_714);
    five_hour.remaining_nano = Some(27_386_721_535);
    five_hour.low_nano = Some(20_000_244_158);
    five_hour.high_nano = Some(81_204_166_575);
    five_hour.remaining_low_nano = Some(15_000_183_118);
    five_hour.remaining_high_nano = Some(60_903_124_931);
    five_hour.observed_spend_nano = 61_448_500;
    five_hour.observed_fraction_units = 168_280;
    five_hour.source = "workload_blend";
    five_hour.confidence = 0.123;
    five_hour.samples = 2;
    let profiles = gemini_profile_values(&status, true, 105);
    assert_eq!(profiles[0]["windows"][0]["cap_usd"], 36.515629);
    assert_eq!(profiles[0]["windows"][0]["capacity_nano"], "36515628714");
    assert_eq!(profiles[0]["windows"][0]["remaining_usd"], 27.386722);
    assert_eq!(profiles[0]["windows"][0]["low_usd"], 20.000244);
    assert_eq!(profiles[0]["windows"][0]["high_usd"], 81.204167);
    assert_eq!(profiles[0]["windows"][0]["observed_spend_nano"], "61448500");
    assert_eq!(profiles[0]["windows"][0]["source"], "workload_blend");
    assert_eq!(profiles[0]["windows"][0]["workload_dependent"], true);
    assert!(profiles[0]["windows"][1]["cap_usd"].is_null());
    let totals = gemini_window_total_values(&status, true, 105);
    assert_eq!(totals[0]["measured_profiles"], 1);
    assert_eq!(totals[0]["capacity_nano"], "36515628714");
    assert_eq!(totals[0]["remaining_nano"], "27386721535");
    assert_eq!(totals[0]["low_usd"], 20.000244);
    assert_eq!(totals[0]["high_usd"], 81.204167);
    assert_eq!(totals[0]["source"], "workload_blend");
    assert_eq!(totals[1]["measured_profiles"], 0);
}

#[test]
fn gemini_profile_persistence_failure_hides_stale_dollar_capacity_everywhere() {
    let mut status = unknown_gemini_status();
    let five_hour = &mut status.profiles[0].capacities[0];
    five_hour.capacity_nano = Some(50_000_000_000);
    five_hour.remaining_nano = Some(30_000_000_000);
    five_hour.low_nano = Some(40_000_000_000);
    five_hour.high_nano = Some(60_000_000_000);
    five_hour.remaining_low_nano = Some(24_000_000_000);
    five_hour.remaining_high_nano = Some(36_000_000_000);
    five_hour.cap_usd = Some(50.0);
    five_hour.remaining_usd = Some(30.0);
    five_hour.low_usd = Some(40.0);
    five_hour.high_usd = Some(60.0);
    five_hour.remaining_low_usd = Some(24.0);
    five_hour.remaining_high_usd = Some(36.0);
    five_hour.source = "workload_blend";
    status.profiles[0].calibration_persistence_ok = false;
    let delivery = forward::GeminiCalibrationDeliveryStatus {
        pending_events: 0,
        dropped_events: 0,
        persistence_ok: true,
        queue_limit: 4_096,
    };

    assert!(!gemini_calibration_persistence_ok(&status, Some(delivery)));

    let profiles = gemini_profile_values(&status, false, 105);
    assert!(profiles[0]["windows"][0]["capacity_nano"].is_null());
    assert!(profiles[0]["windows"][0]["remaining_nano"].is_null());
    assert!(profiles[0]["windows"][0]["cap_usd"].is_null());
    assert_eq!(profiles[0]["windows"][0]["source"], "unknown");
    assert_eq!(profiles[0]["windows"][0]["used_fraction_units"], 25_000_000);

    let totals = gemini_window_total_values(&status, false, 105);
    assert_eq!(totals[0]["observed_profiles"], 1);
    assert_eq!(totals[0]["measured_profiles"], 0);
    assert!(totals[0]["capacity_nano"].is_null());
    assert!(totals[0]["remaining_nano"].is_null());

    let mut body = String::new();
    write_gemini_profile_capacity_metrics(&mut body, &status.profiles[0], false);
    assert!(body.contains("source=\"unknown\"} 0"));
    assert!(!body.contains("claude_api_gemini_profile_window_capacity_usd{"));
    assert!(!body.contains("claude_api_gemini_profile_window_remaining_usd{"));
}

#[test]
fn gemini_non_routable_profile_keeps_quota_but_is_excluded_from_saleable_capacity() {
    let mut status = unknown_gemini_status();
    let five_hour = &mut status.profiles[0].capacities[0];
    five_hour.capacity_nano = Some(50_000_000_000);
    five_hour.remaining_nano = Some(30_000_000_000);
    five_hour.cap_usd = Some(50.0);
    five_hour.remaining_usd = Some(30.0);
    five_hour.source = "workload_blend";
    status.profiles[0].authenticated = false;
    status.available = 0;

    let profiles = gemini_profile_values(&status, true, 105);
    assert_eq!(profiles[0]["windows"][0]["used_fraction_units"], 25_000_000);
    assert!(profiles[0]["windows"][0]["capacity_nano"].is_null());
    assert!(profiles[0]["windows"][0]["remaining_nano"].is_null());
    assert_eq!(profiles[0]["windows"][0]["source"], "unknown");

    let totals = gemini_window_total_values(&status, true, 105);
    assert!(totals.is_empty());
}

#[test]
fn prometheus_omits_unmeasured_gemini_dollar_series() {
    let profile = &unknown_gemini_status().profiles[0];
    let mut body = String::new();
    write_gemini_profile_capacity_metrics(&mut body, profile, true);
    assert!(body.contains(
        "claude_api_gemini_profile_window_estimate_available{profile=\"profile-opaque\",window=\"5h\",window_minutes=\"300\",source=\"unknown\"} 0"
    ));
    assert!(body.contains(
        "claude_api_gemini_profile_window_estimate_available{profile=\"profile-opaque\",window=\"weekly\",window_minutes=\"10080\",source=\"unknown\"} 0"
    ));
    assert!(!body.contains("claude_api_gemini_profile_window_capacity_usd{"));
    assert!(!body.contains("claude_api_gemini_profile_window_remaining_usd{"));
}

fn admin_auth_test_app() -> AppState {
    let mut cfg = crate::config::Settings::from_env().proxy;
    cfg.api_keys = vec!["admin-key".to_string()];
    cfg.control_keys = vec!["control-key".to_string()];
    cfg.panel_keys = vec!["panel-key".to_string()];
    cfg.trust_loopback = false;

    let clients = Arc::new(forward::Clients::new(&cfg));
    AppState {
        provider: forward::ProviderMode::Combined,
        cfg: Arc::new(cfg),
        authority: Arc::new(registry::authority::AuthorityConfig::new(
            ":memory:".to_string(),
            None,
        )),
        data_db_path: Arc::new(":memory:".to_string()),
        pool: Arc::new(pool::Pool::new(
            Vec::new(),
            pool::Reserve::new(0.1, 0.03, 0.02),
            0.0,
            0.0,
        )),
        affinity: Arc::new(forward::AffinityStore::new(None, None, 3_600, 300, 35).unwrap()),
        clients,
        codex: None,
        gemini: None,
        kimi: None,
        glm: None,
        tripo3d: None,
        suno: None,
        billing: None,
        authority_ready: Arc::new(AtomicBool::new(true)),
        breaker: Arc::new(forward::Breaker::new(0)),
        metrics: Arc::new(Metrics::new()),
        probe_poke: None,
        admin_changes: tokio::sync::broadcast::channel(16).0,
    }
}

fn provider_test_app(provider: forward::ProviderMode) -> AppState {
    let mut app = admin_auth_test_app();
    app.provider = provider;
    app
}

/// A permanent input error must never be dressed as a retryable one. Hiding a profile that is not
/// disabled can never succeed, so answering 503 "temporarily unavailable, please retry" would send
/// an operator into a retry loop against a request the engine will always reject — the same wrong
/// error class that turned unsupported models into 529s on the Anthropic plane.
#[tokio::test]
async fn hiding_a_profile_that_is_not_disabled_is_a_client_error_not_a_retryable_one() {
    let service = router(
        provider_test_app(forward::ProviderMode::Gemini),
        Arc::new(AtomicBool::new(true)),
    );
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
    let mut request = Request::builder()
        .method("POST")
        .uri("/gemini-subs/gemini_oauth_000001/disabled")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"disabled":false,"hidden":true}"#))
        .unwrap();
    request.extensions_mut().insert(peer);
    request
        .headers_mut()
        .insert("x-api-key", "control-key".parse().unwrap());

    let status = service.oneshot(request).await.unwrap().status();
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn every_admin_route_enforces_the_control_key_lattice() {
    assert_eq!(ADMIN_ROUTE_CASES.len(), 20);
    let service = router(admin_auth_test_app(), Arc::new(AtomicBool::new(true)));
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));

    for (method, path) in ADMIN_ROUTE_CASES {
        for (credential, expect_unauthorized) in [
            (None, true),
            (Some("panel-key"), true),
            (Some("control-key"), false),
            (Some("admin-key"), false),
        ] {
            let mut request = Request::builder()
                .method(method.clone())
                .uri(*path)
                .body(Body::empty())
                .unwrap();
            request.extensions_mut().insert(peer);
            if let Some(key) = credential {
                request
                    .headers_mut()
                    .insert("x-api-key", key.parse().unwrap());
            }

            let status = service.clone().oneshot(request).await.unwrap().status();
            if expect_unauthorized {
                assert_eq!(
                    status,
                    StatusCode::UNAUTHORIZED,
                    "{method} {path} accepted credential {credential:?}"
                );
            } else {
                assert_ne!(
                    status,
                    StatusCode::UNAUTHORIZED,
                    "{method} {path} rejected credential {credential:?}"
                );
            }
        }
    }
}

#[tokio::test]
async fn admin_event_feed_is_protected_and_starts_with_resync_on_every_plane() {
    use futures_util::StreamExt as _;

    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
    for provider in [
        forward::ProviderMode::Combined,
        forward::ProviderMode::Anthropic,
        forward::ProviderMode::OpenAi,
        forward::ProviderMode::Gemini,
        forward::ProviderMode::Kimi,
    ] {
        let service = router(provider_test_app(provider), Arc::new(AtomicBool::new(true)));
        let mut unauthorized = Request::builder()
            .uri("/admin-events")
            .body(Body::empty())
            .unwrap();
        unauthorized.extensions_mut().insert(peer);
        assert_eq!(
            service
                .clone()
                .oneshot(unauthorized)
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED,
            "{provider:?} exposed the event stream without a control credential"
        );

        let mut request = Request::builder()
            .uri("/admin-events")
            .header("x-api-key", "control-key")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        let response = service.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{provider:?}");
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/event-stream")
        );

        // The body is intentionally infinite. Read only the eager first frame so this proves
        // reconnect safety without waiting for a keepalive or EOF.
        let mut body = response.into_body().into_data_stream();
        let first = tokio::time::timeout(std::time::Duration::from_secs(1), body.next())
            .await
            .expect("initial SSE frame timed out")
            .expect("SSE body ended before its initial frame")
            .expect("initial SSE frame failed");
        let first = std::str::from_utf8(&first).unwrap();
        assert!(first.contains("event: resync"), "{provider:?}: {first}");
        assert!(
            first.contains("\"source\":\"engine\""),
            "{provider:?}: {first}"
        );
        assert!(first.contains("\"resync\":true"), "{provider:?}: {first}");
    }
}

#[tokio::test]
async fn tariff_override_route_validates_before_touching_the_authority() {
    let (app, dir) = billing_test_app("tariff_override_validation");
    let service = router(app, Arc::new(AtomicBool::new(true)));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let valid_payload = json!({
        "input": "5000",
        "output": "25000",
        "cache_read": "500",
        "cache_write_5m": "6250",
        "cache_write_1h": "10000"
    });
    let valid_body = json!({
        "tariff_family": "anthropic/standard/opus-current",
        "effective_from": now,
        "payload": valid_payload,
        "created_by": "operator-test",
        "reason": "route test"
    });

    // Unknown field → 422 (deny_unknown_fields).
    let mut unknown_field = valid_body.clone();
    unknown_field["version"] = json!(2);
    let (status, _) = control_json_request(
        &service,
        Method::POST,
        "/admin/pricing/tariffs/override",
        unknown_field,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Malformed family → 400 invalid.
    let mut bad_family = valid_body.clone();
    bad_family["tariff_family"] = json!("Anthropic/Standard");
    let (status, body) = control_json_request(
        &service,
        Method::POST,
        "/admin/pricing/tariffs/override",
        bad_family,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["result"], "rejected");
    assert_eq!(body["code"], "invalid");

    // JSON-number money legs are not the canonical string encoding → 400 invalid.
    let mut bad_payload = valid_body.clone();
    bad_payload["payload"]["input"] = json!(5000);
    let (status, body) = control_json_request(
        &service,
        Method::POST,
        "/admin/pricing/tariffs/override",
        bad_payload,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid");

    // A payload parsed against the wrong family prefix → 400 invalid.
    let mut wrong_schema = valid_body.clone();
    wrong_schema["payload"] = json!({"input": "1"});
    let (status, _) = control_json_request(
        &service,
        Method::POST,
        "/admin/pricing/tariffs/override",
        wrong_schema,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // effective_from older than the clock-skew grace → 400 invalid.
    let mut stale = valid_body.clone();
    stale["effective_from"] = json!(now - 120);
    let (status, body) = control_json_request(
        &service,
        Method::POST,
        "/admin/pricing/tariffs/override",
        stale,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid");

    // Empty operator attribution → 400 invalid.
    let mut no_operator = valid_body.clone();
    no_operator["created_by"] = json!("  ");
    let (status, _) = control_json_request(
        &service,
        Method::POST,
        "/admin/pricing/tariffs/override",
        no_operator,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A fully valid request reaches the authority; the SQLite fallback is not the tariff
    // override authority, so it answers with the same unavailable class as other PG-only
    // pricing routes.
    let (status, body) = control_json_request(
        &service,
        Method::POST,
        "/admin/pricing/tariffs/override",
        valid_body,
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"], "billing authority unavailable");

    // A multi-epoch compiled family fails before the authority. A zero-time v2 seed cannot
    // represent its history even after the final cutoff has passed.
    let (status, body) = control_json_request(
        &service,
        Method::POST,
        "/admin/pricing/tariffs/seed",
        json!({
            "created_by": "operator-test",
            "reason": "route test",
            "tariff_family": "google/gemini/gemini-3.6-flash"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid");
    assert!(body["reason"]
        .as_str()
        .unwrap()
        .contains("multi-epoch compiled schedules"));

    let (status, body) = control_json_request(
        &service,
        Method::POST,
        "/admin/pricing/tariffs/seed",
        json!({
            "created_by": "operator-test",
            "reason": "all-family route test"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid");
    assert!(body["reason"]
        .as_str()
        .unwrap()
        .contains("google/gemini/gemini-3.6-flash"));

    let (status, body) = control_json_request(
        &service,
        Method::POST,
        "/admin/pricing/tariffs/seed",
        json!({
            "created_by": "operator-test",
            "reason": "3.7 route test",
            "tariff_family": "google/gemini/gemini-3.7-flash"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid");
    assert!(body["reason"]
        .as_str()
        .unwrap()
        .contains("multi-epoch compiled schedules"));

    drop(service);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn tariff_seed_rejects_unknown_families_and_requires_the_pg_authority() {
    let (app, dir) = billing_test_app("tariff_seed_validation");
    let service = router(app, Arc::new(AtomicBool::new(true)));

    // A family the compiled catalog does not know → 400 invalid, before any authority call.
    let (status, body) = control_json_request(
        &service,
        Method::POST,
        "/admin/pricing/tariffs/seed",
        json!({
            "created_by": "operator-test",
            "reason": "route test",
            "tariff_family": "anthropic/standard/not-a-family"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid");

    // Missing attribution → 400 invalid.
    let (status, _) = control_json_request(
        &service,
        Method::POST,
        "/admin/pricing/tariffs/seed",
        json!({"created_by": "", "reason": "route test"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A known family is accepted by validation and then fails only on the SQLite authority.
    let (status, body) = control_json_request(
        &service,
        Method::POST,
        "/admin/pricing/tariffs/seed",
        json!({
            "created_by": "operator-test",
            "reason": "route test",
            "tariff_family": "openai/gpt-image-2"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"], "billing authority unavailable");

    // Listing overrides likewise requires the PostgreSQL authority.
    let (status, body) =
        control_json_request(&service, Method::GET, "/admin/pricing/tariffs", Value::Null).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"], "billing authority unavailable");

    drop(service);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn compiled_tariff_catalog_is_read_only_and_built_from_metering() {
    // No billing at all: the compiled dump must not need the authority.
    let service = router(admin_auth_test_app(), Arc::new(AtomicBool::new(true)));
    let (status, body) = control_json_request(
        &service,
        Method::GET,
        "/admin/pricing/tariffs/compiled",
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let families = body["families"].as_array().expect("families array");
    assert!(
        families.len() >= 30,
        "compiled families: {}",
        families.len()
    );
    let by_family: std::collections::BTreeMap<&str, &Value> = families
        .iter()
        .map(|entry| (entry["tariff_family"].as_str().unwrap(), entry))
        .collect();
    assert_eq!(
        by_family.len(),
        families.len(),
        "families are unique and sorted"
    );

    // Representative families from every provider, with i128 legs as canonical decimal strings.
    for family in [
        "anthropic/standard/opus-current",
        "anthropic/fast/opus-4-7-conservative",
        "openai/codex/gpt-5.6-sol",
        "chatgpt/codex-credits/gpt-5.6-sol",
        "google/gemini/gemini-2.5-pro",
        "zhipu/glm/glm-5.2",
        "zhipu/glm-credits/glm-5.2",
        "moonshot/kimi/kimi-k3",
        "openai/gpt-image-2",
    ] {
        let entry = by_family
            .get(family)
            .unwrap_or_else(|| panic!("compiled catalog is missing {family}"));
        assert!(
            entry["payload"].is_object(),
            "{family} payload must be an object"
        );
        assert!(entry["has_future_epoch"].is_boolean());
        assert!(entry["seed_safe"].is_boolean());
    }
    let opus = &by_family["anthropic/standard/opus-current"]["payload"];
    assert_eq!(opus["input"], "5000");
    assert_eq!(opus["output"], "25000");
    assert!(opus["input"].is_string(), "money legs are decimal strings");
    let glm_credits = &by_family["zhipu/glm-credits/glm-5.2"]["payload"];
    assert_eq!(glm_credits["input_tenths"], "69");
    let image = &by_family["openai/gpt-image-2"]["payload"];
    assert_eq!(image["image_output"], "30000");
    let gemini = &by_family["google/gemini/gemini-2.5-pro"]["payload"];
    assert_eq!(gemini["search"]["kind"], "per_grounded_prompt");

    let compiled_ts = body["compiled_ts"].as_i64().expect("compiled_ts");
    let gemini_36 = by_family["google/gemini/gemini-3.6-flash"];
    let promo_active = compiled_ts < 1_798_761_600;
    assert_eq!(gemini_36["has_future_epoch"], promo_active);
    assert_eq!(gemini_36["seed_safe"], false);
    assert_eq!(
        gemini_36["payload"]["input"],
        if promo_active { "750" } else { "1500" }
    );
    assert_eq!(
        gemini_36["payload"]["cached_input"],
        if promo_active { "75" } else { "150" }
    );
    assert_eq!(
        gemini_36["payload"]["output"],
        if promo_active { "3750" } else { "7500" }
    );
    assert_eq!(gemini_36["payload"]["search"]["nano"], "14000000");

    let gemini_37 = by_family["google/gemini/gemini-3.7-flash"];
    assert_eq!(gemini_37["has_future_epoch"], promo_active);
    assert_eq!(gemini_37["seed_safe"], false);
    assert_eq!(
        gemini_37["payload"]["input"],
        if promo_active { "750" } else { "1500" }
    );
    assert_eq!(
        gemini_37["payload"]["cached_input"],
        if promo_active { "75" } else { "150" }
    );
    assert_eq!(
        gemini_37["payload"]["output"],
        if promo_active { "3750" } else { "7500" }
    );
    assert_eq!(gemini_37["payload"]["search"]["nano"], "14000000");

    // Sonnet 5 intro pricing is time-bounded: the family is published only while the compiled
    // epoch has not flipped (2026-09-01T00:00:00Z = 1788220800).
    assert_eq!(
        by_family.contains_key("anthropic/standard/sonnet-5-intro"),
        compiled_ts < 1_788_220_800,
        "sonnet-5-intro presence must follow the compiled epoch"
    );
}

async fn control_json_request(
    service: &Router,
    method: Method,
    path: &str,
    body: Value,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("x-api-key", "control-key")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424))));
    let response = service.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 256 * 1024).await.unwrap();
    let value = serde_json::from_slice(&body)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into_owned()));
    (status, value)
}

#[tokio::test]
async fn account_and_ledger_control_reads_expose_balance_and_provider_evidence() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "claude-api-read-surfaces-{}-{nonce}.sqlite",
        std::process::id()
    ));
    let connection = registry::open(path.to_string_lossy().as_ref()).unwrap();
    connection
        .execute_batch(
            "INSERT INTO accounts(
                 id,handle,balance_nano,spent_nano,reserved_nano,mult_bp,status,
                 created_ts,created
             ) VALUES('acct_read_surface','read-user',900,300,40,5000,'active',1,'');
             INSERT INTO ledger(
                 account_id,key,kind,request_id,amount_nano,ref,balance_after_nano,ts,model,
                 provider,official_nano,uncollected_nano
             ) VALUES(
                 'acct_read_surface','read-key','charge','read-request',300,'provider:read',900,
                 2,'claude-read','anthropic',600,70
             );",
        )
        .unwrap();
    drop(connection);

    let billing =
        Arc::new(forward::AsyncBilling::start(path.to_string_lossy().into_owned(), 1).unwrap());
    let mut app = admin_auth_test_app();
    app.billing = Some(billing);
    let service = router(app, Arc::new(AtomicBool::new(true)));

    let (status, account) = control_json_request(
        &service,
        Method::GET,
        "/admin/account/acct_read_surface",
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(account["account"], "acct_read_surface");
    assert_eq!(account["balance_nano"], 900);
    assert_eq!(account["spent_nano"], 300);
    assert_eq!(account["reserved_nano"], 40);
    assert_eq!(account["mult_bp"], 5000);
    assert_eq!(account["status"], "active");
    // The retired funding split is gone: one balance is the whole answer.
    assert!(account.get("funding").is_none());

    let (status, ledger) = control_json_request(
        &service,
        Method::GET,
        "/admin/account/acct_read_surface/ledger?after_id=0&limit=10",
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let entry = &ledger["entries"][0];
    assert_eq!(entry["request_id"], "read-request");
    assert_eq!(entry["provider"], "anthropic");
    assert_eq!(entry["official_nano"], 600);
    assert_eq!(entry["amount_nano"], 300);
    assert_eq!(entry["uncollected_nano"], 70);
    assert_eq!(entry["model"], "claude-read");
    assert!(entry.get("attribution").is_none());
    assert!(entry.get("funding_allocations").is_none());

    let _ = std::fs::remove_file(path);
}

fn capacity(email: &str, available: f64, routable: bool, calibrated: bool) -> pool::Cap {
    pool::Cap {
        email: email.to_string(),
        plan: "max20".to_string(),
        calibrated,
        util5h: 0.0,
        util7d: 0.0,
        quota5h: None,
        quota7d: None,
        reset5h_in: 0,
        reset7d_in: 0,
        cap5h_usd: available,
        cap7d_usd: available,
        rem5h_usd: available,
        rem7d_usd: available,
        avail_1h_usd: available,
        avail_5h_usd: available,
        avail_1d_usd: available,
        avail_7d_usd: available,
        status: String::new(),
        cooling: !routable,
        routable,
        auth_dead: false,
        auth_state: "healthy".to_string(),
        dead_reason: String::new(),
        dead_since_ts: 0,
    }
}

fn claude_calibration(
    email: &str,
    window_kind: &str,
    used_fraction_units: i64,
    observed_fraction_units: i64,
    observed_spend_nano: i64,
) -> registry::AnthropicCalibrationRow {
    let duration = if window_kind == "5h" { 300 } else { 10_080 };
    let capacity = i64::try_from(
        i128::from(observed_spend_nano) * CLAUDE_FRACTION_SCALE
            / i128::from(observed_fraction_units),
    )
    .unwrap();
    registry::AnthropicCalibrationRow {
        subject_id: email.to_owned(),
        plan: "max20".to_owned(),
        window_kind: window_kind.to_owned(),
        window_duration_mins: duration,
        resets_at: 2_000_000_000 + duration * 60,
        anchor_used_fraction_units: used_fraction_units,
        anchor_resolution_fraction_units: 100_000,
        anchor_spend_nano: observed_spend_nano,
        used_fraction_units,
        measurement_resolution_fraction_units: 100_000,
        observed_at: 100,
        observed_fraction_units,
        observed_spend_nano,
        samples: 2,
        unattributed_fraction_units: 0,
        current_capacity_nano: Some(capacity),
        current_low_nano: Some(capacity - 1),
        current_high_nano: Some(capacity + 1),
        current_confidence_bp: 8_000,
        last_measured_at: Some(100),
        estimator_version: 1,
        version: 1,
        updated_ts: 100,
    }
}

fn claude_delivery(pending_events: usize) -> Option<forward::AnthropicCalibrationDeliveryStatus> {
    Some(forward::AnthropicCalibrationDeliveryStatus {
        pending_events,
        dropped_events: 0,
        persistence_ok: pending_events == 0,
        queue_limit: 4_096,
    })
}

/// Delivery that drains without backlog but cannot persist: the second fail-closed money path.
fn claude_degraded_delivery() -> Option<forward::AnthropicCalibrationDeliveryStatus> {
    Some(forward::AnthropicCalibrationDeliveryStatus {
        pending_events: 0,
        dropped_events: 1,
        persistence_ok: false,
        queue_limit: 4_096,
    })
}

#[test]
fn claude_recent_turn_contract_masks_subject_and_preserves_exact_vector() {
    let event = registry::ProviderTurnCalibrationEvent {
        provider: registry::PROVIDER_ANTHROPIC.to_owned(),
        request_id: "cal-request-1".to_owned(),
        subject_id: "operator@example.test".to_owned(),
        model_id: "claude-opus-4-8".to_owned(),
        service_tier: "fast".to_owned(),
        inference_geo: "global".to_owned(),
        tariff_schedule_id: "anthropic/test/v1".to_owned(),
        priced_ts: 99,
        completed_at: 100,
        input_tokens: 11,
        audio_input_tokens: 0,
        cache_read_tokens: 12,
        cached_audio_input_tokens: 0,
        cache_write_5m_tokens: 13,
        cache_write_1h_tokens: 14,
        output_tokens: 15,
        thinking_output_tokens: 0,
        image_output_tokens: 0,
        tool_prompt_tokens: 0,
        search_queries: 1,
        grounded_search_prompts: 0,
        api_input_nanousd: 110,
        api_audio_input_nanousd: 0,
        api_cache_read_nanousd: 12,
        api_cached_audio_input_nanousd: 0,
        api_cache_write_5m_nanousd: 130,
        api_cache_write_1h_nanousd: 280,
        api_output_nanousd: 750,
        api_image_output_nanousd: 0,
        api_search_nanousd: 10_000_000,
        api_total_nanousd: 10_001_282,
    };

    let value = anthropic_calibration_event_value(&event);
    assert_eq!(value["request_id"], "cal-request-1");
    assert_eq!(value["email"], "oper…");
    assert!(value.get("subject_id").is_none());
    assert_eq!(value["cache_write_1h_tokens"], "14");
    assert_eq!(value["api_total_nanousd"], "10001282");
}

#[test]
fn gemini_recent_turn_contract_preserves_every_runner_vector_field() {
    let event = registry::ProviderTurnCalibrationEvent {
        provider: registry::PROVIDER_GOOGLE.to_owned(),
        request_id: "00000000-0000-4000-8000-000000000001".to_owned(),
        subject_id: "profile-opaque".to_owned(),
        model_id: "gemini-3.6-flash".to_owned(),
        service_tier: "standard".to_owned(),
        inference_geo: "global".to_owned(),
        tariff_schedule_id: "google/test/v1".to_owned(),
        priced_ts: 99,
        completed_at: 100,
        input_tokens: 11,
        audio_input_tokens: 12,
        cache_read_tokens: 13,
        cached_audio_input_tokens: 3,
        cache_write_5m_tokens: 0,
        cache_write_1h_tokens: 0,
        output_tokens: 15,
        thinking_output_tokens: 5,
        image_output_tokens: 16,
        tool_prompt_tokens: 4,
        search_queries: 2,
        grounded_search_prompts: 1,
        api_input_nanousd: 110,
        api_audio_input_nanousd: 120,
        api_cache_read_nanousd: 13,
        api_cached_audio_input_nanousd: 3,
        api_cache_write_5m_nanousd: 0,
        api_cache_write_1h_nanousd: 0,
        api_output_nanousd: 750,
        api_image_output_nanousd: 960,
        api_search_nanousd: 28_000_000,
        api_total_nanousd: 28_001_956,
    };

    let value = gemini_calibration_event_value(&event);
    assert_eq!(value["request_id"], event.request_id);
    assert_eq!(value["profile_id"], "profile-opaque");
    assert!(value.get("subject_id").is_none());
    assert_eq!(value["cache_write_5m_tokens"], "0");
    assert_eq!(value["cache_write_1h_tokens"], "0");
    assert_eq!(value["api_cache_write_5m_nanousd"], "0");
    assert_eq!(value["api_cache_write_1h_nanousd"], "0");
    assert_eq!(value["thinking_output_tokens"], "5");
    assert_eq!(value["tool_prompt_tokens"], "4");
    assert_eq!(value["api_total_nanousd"], "28001956");
}

#[test]
fn claude_same_plan_capacity_is_pooled_and_unroutable_supply_is_excluded() {
    let caps = vec![
        capacity("first@example.test", 1.0, true, false),
        capacity("second@example.test", 999.0, true, true),
        capacity("cooling@example.test", 50_000.0, false, true),
    ];
    let rows = vec![
        claude_calibration(
            "first@example.test",
            "5h",
            10_000_000,
            10_000_000,
            1_000_000_000,
        ),
        claude_calibration(
            "second@example.test",
            "5h",
            50_000_000,
            20_000_000,
            4_000_000_000,
        ),
        claude_calibration(
            "first@example.test",
            "7d",
            20_000_000,
            10_000_000,
            10_000_000_000,
        ),
        claude_calibration(
            "second@example.test",
            "7d",
            30_000_000,
            20_000_000,
            20_000_000_000,
        ),
    ];
    let report = (rows, Vec::new(), Vec::new());
    let value = capacity_value(&caps, Some(&report), claude_delivery(0), 100);

    assert_eq!(value["per_sub"][0]["cap5h_nano"], "16666666667");
    assert_eq!(value["per_sub"][1]["cap5h_nano"], "16666666667");
    assert_eq!(value["per_sub"][2]["cap5h_nano"], "16666666667");
    assert_eq!(value["window_totals"][0]["capacity_nano"], "33333333334");
    assert_eq!(value["window_totals"][0]["remaining_nano"], "23333333334");
    assert_eq!(value["window_totals"][0]["routable_subs"], 2);
    assert_eq!(value["window_totals"][0]["calibrated_subs"], 2);
    assert_eq!(value["window_totals"][1]["capacity_nano"], "200000000000");
    assert_eq!(value["window_totals"][1]["remaining_nano"], "150000000000");
    assert_eq!(value["plan_cohorts"][0]["same_plan_capacity"], true);
    assert_eq!(
        value["capacity_semantics"]["legacy_pool_prior_authoritative"],
        false
    );
}

#[test]
fn claude_fresh_runtime_quota_without_reset_publishes_current_dollars_only() {
    let now = 2_000;
    let mut cap = capacity("runtime@example.test", 999.0, true, true);
    cap.quota5h = Some(pool::QuotaSnapshot {
        used_fraction_units: 25_000_000,
        measurement_resolution_fraction_units: 100_000,
        observed_at: now - 50,
        resets_at: None,
    });
    cap.quota7d = Some(pool::QuotaSnapshot {
        used_fraction_units: 75_000_000,
        measurement_resolution_fraction_units: 100_000,
        observed_at: now - 50,
        resets_at: None,
    });
    let report = (
        vec![
            claude_calibration(
                "runtime@example.test",
                "5h",
                10_000_000,
                10_000_000,
                1_000_000_000,
            ),
            claude_calibration(
                "runtime@example.test",
                "7d",
                10_000_000,
                10_000_000,
                10_000_000_000,
            ),
        ],
        Vec::new(),
        Vec::new(),
    );

    let value = capacity_value(&[cap], Some(&report), claude_delivery(0), now);
    let five = &value["per_sub"][0]["windows"][0];
    assert_eq!(five["remaining_nano"], "7500000000");
    assert_eq!(five["snapshot_fresh"], true);
    assert_eq!(five["current_quota_source"], "runtime_quota_snapshot");
    assert!(five["resets_at"].is_null());
    assert!(value["per_sub"][0]["reset5h_in"].is_null());
    assert_eq!(value["per_sub"][0]["rem5h_nano"], "7500000000");
    assert_eq!(value["per_sub"][0]["rem7d_nano"], "25000000000");
    assert_eq!(value["window_totals"][0]["remaining_nano"], "7500000000");
    assert_eq!(value["window_totals"][1]["remaining_nano"], "25000000000");
    assert!(value["available_nano"]["next_5h"].is_null());
}

#[test]
fn claude_stale_runtime_quota_does_not_reopen_current_supply() {
    let now = 2_000;
    let mut cap = capacity("stale@example.test", 999.0, true, true);
    cap.quota5h = Some(pool::QuotaSnapshot {
        used_fraction_units: 25_000_000,
        measurement_resolution_fraction_units: 100_000,
        observed_at: now - CLAUDE_SNAPSHOT_MAX_AGE_SECS - 1,
        resets_at: Some(now + 1_800),
    });
    let report = (
        vec![claude_calibration(
            "stale@example.test",
            "5h",
            10_000_000,
            10_000_000,
            1_000_000_000,
        )],
        Vec::new(),
        Vec::new(),
    );

    let value = capacity_value(
        std::slice::from_ref(&cap),
        Some(&report),
        claude_delivery(0),
        now,
    );
    let five = &value["per_sub"][0]["windows"][0];
    assert!(five["remaining_nano"].is_null());
    assert_eq!(five["snapshot_fresh"], false);
    assert_eq!(five["used_fraction_units"], 25_000_000);
    assert_eq!(five["resets_at"], now + 1_800);
    assert_eq!(five["last_known_remaining_nano"], "7500000000");
    assert_eq!(five["last_known_quota_source"], "runtime_quota_snapshot");
    assert!(five["current_quota_source"].is_null());
    assert_eq!(five["missing_reason"], "stale_current_quota_snapshot");
    assert_eq!(value["per_sub"][0]["reset5h_in"], 1_800);
    assert!(value["window_totals"][0]["remaining_nano"].is_null());

    let after_reset = capacity_value(
        std::slice::from_ref(&cap),
        Some(&report),
        claude_delivery(0),
        now + 1_801,
    );
    // After the provider deadline the stale fraction is never carried into the new window. The
    // window is now empty by construction, so a healthy routable subscription publishes an exact
    // zero instead of an unmeasured `null`; money stays closed until a real snapshot prices it.
    let expired = &after_reset["per_sub"][0]["windows"][0];
    assert_eq!(expired["used_fraction_units"], 0);
    assert_eq!(expired["quota_state"], "window_rolled_over");
    assert_eq!(
        expired["displayed_quota_source"],
        "provider_window_rollover"
    );
    assert!(expired["resets_at"].is_null());
    assert!(expired["remaining_nano"].is_null());
    assert!(expired["last_known_remaining_nano"].is_null());
    assert!(expired["last_known_quota_source"].is_null());
    assert!(expired["current_quota_source"].is_null());
    assert_eq!(expired["snapshot_fresh"], false);
    assert!(after_reset["per_sub"][0]["reset5h_in"].is_null());
    assert!(after_reset["window_totals"][0]["remaining_nano"].is_null());

    // A rolled-over window of an unhealthy subscription keeps the old fail-closed silence: its
    // reset is no evidence that quota was refilled for us.
    let mut dead = cap.clone();
    dead.auth_state = "dead".to_owned();
    dead.auth_dead = true;
    dead.routable = false;
    let dead_value = capacity_value(&[dead], Some(&report), claude_delivery(0), now + 1_801);
    let dead_five = &dead_value["per_sub"][0]["windows"][0];
    assert!(dead_five["used_fraction_units"].is_null());
    assert_eq!(dead_five["quota_state"], "awaiting_probe");
}

#[test]
fn claude_cooling_preserves_provider_reset_after_quota_snapshot_stales() {
    let now = 2_000;
    let mut cap = capacity("cooling@example.test", 999.0, false, true);
    cap.util5h = 1.0;
    cap.util7d = 0.42;
    cap.reset5h_in = 1_800;
    cap.reset7d_in = 86_400;
    cap.quota5h = Some(pool::QuotaSnapshot {
        used_fraction_units: 100_000_000,
        measurement_resolution_fraction_units: 100_000,
        observed_at: now - CLAUDE_SNAPSHOT_MAX_AGE_SECS - 1,
        resets_at: Some(now + 1_800),
    });
    cap.quota7d = Some(pool::QuotaSnapshot {
        used_fraction_units: 42_000_000,
        measurement_resolution_fraction_units: 100_000,
        observed_at: now - CLAUDE_SNAPSHOT_MAX_AGE_SECS - 1,
        resets_at: Some(now + 86_400),
    });
    let report = (
        vec![claude_calibration(
            "cooling@example.test",
            "5h",
            100_000_000,
            10_000_000,
            1_000_000_000,
        )],
        Vec::new(),
        Vec::new(),
    );

    let value = capacity_value(&[cap], Some(&report), claude_delivery(0), now);
    let five = &value["per_sub"][0]["windows"][0];
    assert_eq!(five["snapshot_fresh"], false);
    assert!(five["remaining_nano"].is_null());
    assert_eq!(five["resets_at"], now + 1_800);
    assert_eq!(five["last_known_remaining_nano"], "0");
    assert_eq!(five["last_known_quota_source"], "runtime_quota_snapshot");
    assert_eq!(value["per_sub"][0]["cooling"], true);
    assert_eq!(value["per_sub"][0]["routable"], false);
    assert_eq!(value["per_sub"][0]["reset5h_in"], 1_800);
    assert_eq!(value["per_sub"][0]["reset7d_in"], 86_400);
    assert!(value["window_totals"][0]["remaining_nano"].is_null());

    let mut unknown_reset = capacity("unknown-reset@example.test", 999.0, false, true);
    unknown_reset.util5h = 1.0;
    unknown_reset.reset5h_in = 18_000;
    unknown_reset.quota5h = Some(pool::QuotaSnapshot {
        used_fraction_units: 100_000_000,
        measurement_resolution_fraction_units: 100_000,
        observed_at: now - CLAUDE_SNAPSHOT_MAX_AGE_SECS - 1,
        resets_at: None,
    });
    let empty_report = (Vec::new(), Vec::new(), Vec::new());
    let unknown = capacity_value(
        &[unknown_reset],
        Some(&empty_report),
        claude_delivery(0),
        now,
    );
    assert!(unknown["per_sub"][0]["reset5h_in"].is_null());
}

#[test]
fn claude_runtime_quota_uses_same_plan_capacity_without_own_durable_row() {
    let now = 100;
    let evidence = capacity("evidence@example.test", 999.0, true, true);
    let mut runtime = capacity("runtime@example.test", 999.0, true, false);
    runtime.quota5h = Some(pool::QuotaSnapshot {
        used_fraction_units: 40_000_000,
        measurement_resolution_fraction_units: 100_000,
        observed_at: now,
        resets_at: None,
    });
    runtime.quota7d = Some(pool::QuotaSnapshot {
        used_fraction_units: 60_000_000,
        measurement_resolution_fraction_units: 100_000,
        observed_at: now,
        resets_at: None,
    });
    let report = (
        vec![
            claude_calibration(
                "evidence@example.test",
                "5h",
                10_000_000,
                10_000_000,
                1_000_000_000,
            ),
            claude_calibration(
                "evidence@example.test",
                "7d",
                20_000_000,
                10_000_000,
                10_000_000_000,
            ),
        ],
        Vec::new(),
        Vec::new(),
    );

    let value = capacity_value(&[evidence, runtime], Some(&report), claude_delivery(0), now);
    let runtime_five = &value["per_sub"][1]["windows"][0];
    assert_eq!(runtime_five["capacity_nano"], "10000000000");
    assert_eq!(runtime_five["remaining_nano"], "6000000000");
    assert_eq!(
        runtime_five["current_quota_source"],
        "runtime_quota_snapshot"
    );
    assert!(runtime_five["missing_reason"].is_null());
    assert_eq!(value["window_totals"][0]["remaining_nano"], "15000000000");
}

#[test]
fn claude_delivery_degradation_remains_fail_closed_with_runtime_quota() {
    let now = 100;
    let mut cap = capacity("runtime@example.test", 999.0, true, true);
    cap.quota5h = Some(pool::QuotaSnapshot {
        used_fraction_units: 25_000_000,
        measurement_resolution_fraction_units: 100_000,
        observed_at: now,
        resets_at: None,
    });
    let report = (
        vec![claude_calibration(
            "runtime@example.test",
            "5h",
            10_000_000,
            10_000_000,
            1_000_000_000,
        )],
        Vec::new(),
        Vec::new(),
    );

    let value = capacity_value(&[cap], Some(&report), claude_delivery(1), now);
    let five = &value["per_sub"][0]["windows"][0];
    // Money stays fail-closed under a pending FIFO...
    assert!(five["remaining_nano"].is_null());
    assert!(five["last_known_remaining_nano"].is_null());
    assert_eq!(five["missing_reason"], "calibration_delivery_pending");
    // ...but the exact provider quota wall remains visible: a dollar-evidence failure must not
    // blind the operator to real utilization, exactly as on the Gemini/KIMI/GLM boards.
    assert_eq!(five["used_fraction_units"], 25_000_000);
    assert_eq!(five["snapshot_fresh"], true);
    assert_eq!(five["current_quota_source"], "runtime_quota_snapshot");
    assert!(five["quota_state"].is_null());
    assert!(value["window_totals"][0]["remaining_nano"].is_null());
    assert!(value["plan_cohorts"][0]["fleet_remaining_nano"].is_null());
    assert_eq!(
        value["plan_cohorts"][0]["missing_reason"],
        "calibration_delivery_pending"
    );
}

#[test]
fn claude_fleet_totals_fail_closed_for_missing_plan_or_authority_evidence() {
    let mut caps = vec![capacity("first@example.test", 999.0, true, true)];
    let report = (
        vec![
            claude_calibration(
                "first@example.test",
                "5h",
                10_000_000,
                10_000_000,
                1_000_000_000,
            ),
            claude_calibration(
                "first@example.test",
                "7d",
                20_000_000,
                10_000_000,
                10_000_000_000,
            ),
        ],
        Vec::new(),
        Vec::new(),
    );
    let measured = capacity_value(&caps, Some(&report), claude_delivery(0), 100);
    assert!(measured["window_totals"][0]["capacity_nano"].is_string());

    let stale = capacity_value(&caps, Some(&report), claude_delivery(0), 2_000);
    assert!(stale["window_totals"][0]["capacity_nano"].is_string());
    assert!(stale["window_totals"][0]["remaining_nano"].is_null());
    assert_eq!(
        stale["per_sub"][0]["windows"][0]["missing_reason"],
        "stale_current_quota_snapshot"
    );

    let mut uncovered = capacity("other@example.test", 50_000.0, true, true);
    uncovered.plan = "max5".to_owned();
    caps.push(uncovered);
    let missing_plan = capacity_value(&caps, Some(&report), claude_delivery(0), 100);
    assert!(missing_plan["window_totals"][0]["capacity_nano"].is_null());
    assert_eq!(
        missing_plan["window_totals"][0]["missing_reason"],
        "missing_plan_evidence"
    );

    let missing_authority = capacity_value(&caps[..1], None, None, 100);
    assert!(missing_authority["window_totals"][0]["capacity_nano"].is_null());
    assert!(missing_authority["available_nano"]["next_5h"].is_null());
    assert_eq!(missing_authority["per_sub"][0]["cap5h_nano"], Value::Null);
    assert_eq!(
        missing_authority["window_totals"][0]["missing_reason"],
        "calibration_authority_unavailable"
    );

    let pending_delivery = capacity_value(&caps[..1], Some(&report), claude_delivery(2), 100);
    assert!(pending_delivery["window_totals"][0]["capacity_nano"].is_string());
    assert!(pending_delivery["window_totals"][0]["remaining_nano"].is_null());
    assert!(pending_delivery["available_nano"]["next_5h"].is_null());
    assert_eq!(
        pending_delivery["per_sub"][0]["windows"][0]["missing_reason"],
        "calibration_delivery_pending"
    );
    assert_eq!(
        pending_delivery["calibration_delivery"]["pending_events"],
        2
    );
}

#[test]
fn readiness_flag_flips_before_drain() {
    let accepting = AtomicBool::new(true);
    let authority_ready = AtomicBool::new(true);
    assert_eq!(
        readiness_snapshot(&accepting, &authority_ready, None, 0),
        (StatusCode::OK, json!({"ready": true, "active_requests": 0}))
    );

    accepting.store(false, Ordering::Release);
    // A draining slot must report what it still owes: this is the number the deployer waits on
    // before stopping the unit, so it has to survive the readiness flip rather than appear only
    // while the slot is healthy.
    assert_eq!(
        readiness_snapshot(&accepting, &authority_ready, Some(true), 3),
        (
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"ready": false, "reason": "draining", "active_requests": 3}),
        )
    );
    accepting.store(true, Ordering::Release);
    authority_ready.store(false, Ordering::Release);
    assert_eq!(
        readiness_snapshot(&accepting, &authority_ready, Some(true), 0),
        (
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"ready": false, "reason": "authority_unavailable", "active_requests": 0}),
        )
    );
    authority_ready.store(true, Ordering::Release);
    assert_eq!(
        readiness_snapshot(&accepting, &authority_ready, Some(false), 0),
        (
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"ready": false, "reason": "provider_unavailable", "active_requests": 0}),
        )
    );
}

#[test]
fn openai_readiness_preserves_a_single_working_home() {
    assert!(codex_provider_ready(1));
    assert!(codex_provider_ready(7));
    assert!(!codex_provider_ready(0));
}

#[test]
fn api_plane_is_hostname_selected_and_auth_header_agnostic() {
    let mut headers = HeaderMap::new();
    assert!(!is_openai_plane(&headers));

    headers.insert(
        axum::http::header::AUTHORIZATION,
        "Bearer customer-key".parse().unwrap(),
    );
    assert!(!is_openai_plane(&headers));

    headers.insert("x-api-key", "customer-key".parse().unwrap());
    headers.insert("anthropic-version", "2023-06-01".parse().unwrap());
    headers.insert("anthropic-beta", "test-feature".parse().unwrap());
    assert!(!is_openai_plane(&headers));

    headers.insert(API_PLANE_HEADER, "anthropic".parse().unwrap());
    assert!(!is_openai_plane(&headers));

    headers.insert(API_PLANE_HEADER, "openai".parse().unwrap());
    assert!(is_openai_plane(&headers));
}

#[tokio::test]
async fn image_routes_authenticate_before_body_parsing_or_gateway_discovery() {
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));

    for provider in [
        forward::ProviderMode::OpenAi,
        forward::ProviderMode::Combined,
    ] {
        for (path, content_type) in [
            ("/v1/images/generations", "application/json"),
            ("/v1/images/edits", "multipart/form-data; boundary=missing"),
        ] {
            let mut request = Request::builder()
                .method(Method::POST)
                .uri(path)
                .header("content-type", content_type)
                .body(Body::from(vec![b'x'; 300 * 1024]))
                .unwrap();
            if provider == forward::ProviderMode::Combined {
                request
                    .headers_mut()
                    .insert(API_PLANE_HEADER, "openai".parse().unwrap());
            }
            request.extensions_mut().insert(peer);

            let response = router(provider_test_app(provider), Arc::new(AtomicBool::new(true)))
                .oneshot(request)
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "{provider:?} {path} parsed or buffered an unauthenticated body"
            );
        }
    }
}

/// The image routes and the image model entries both live behind the OpenAI plane gateway. A
/// deployment without that gateway must answer 404 for either, rather than advertising a model no
/// pool can serve.
#[tokio::test]
async fn image_routes_and_models_require_the_openai_plane_gateway() {
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
    let service = router(
        provider_test_app(forward::ProviderMode::Combined),
        Arc::new(AtomicBool::new(true)),
    );

    let mut marked = Request::builder()
        .method(Method::POST)
        .uri("/v1/images/generations")
        .header("x-api-key", "admin-key")
        .header(API_PLANE_HEADER, "openai")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"model":"gpt-image-2","prompt":"test"}"#))
        .unwrap();
    marked.extensions_mut().insert(peer);
    let marked = service.clone().oneshot(marked).await.unwrap();
    assert_eq!(marked.status(), StatusCode::NOT_FOUND);

    let mut models = Request::builder()
        .uri("/v1/models/gpt-image-2")
        .header("x-api-key", "admin-key")
        .header(API_PLANE_HEADER, "openai")
        .body(Body::empty())
        .unwrap();
    models.extensions_mut().insert(peer);
    let models = service.oneshot(models).await.unwrap();
    assert_eq!(models.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn fixed_provider_routers_ignore_the_legacy_plane_header() {
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));

    let mut anthropic_request = Request::builder()
        .uri("/pool")
        .header("x-api-key", "admin-key")
        .header(API_PLANE_HEADER, "openai")
        .body(Body::empty())
        .unwrap();
    anthropic_request.extensions_mut().insert(peer);
    let anthropic = router(
        provider_test_app(forward::ProviderMode::Anthropic),
        Arc::new(AtomicBool::new(true)),
    )
    .oneshot(anthropic_request)
    .await
    .unwrap();
    assert_eq!(anthropic.status(), StatusCode::OK);

    let mut openai_request = Request::builder()
        .uri("/pool")
        .header("x-api-key", "admin-key")
        .header(API_PLANE_HEADER, "anthropic")
        .body(Body::empty())
        .unwrap();
    openai_request.extensions_mut().insert(peer);
    let openai = router(
        provider_test_app(forward::ProviderMode::OpenAi),
        Arc::new(AtomicBool::new(true)),
    )
    .oneshot(openai_request)
    .await
    .unwrap();
    assert_eq!(openai.status(), StatusCode::NOT_FOUND);

    let mut gemini_request = Request::builder()
        .method(Method::POST)
        .uri("/v1beta/models/gemini-2.5-flash:generateContent")
        .header("x-api-key", "admin-key")
        .header(API_PLANE_HEADER, "openai")
        .body(Body::from(r#"{"contents":[]}"#))
        .unwrap();
    gemini_request.extensions_mut().insert(peer);
    let gemini = router(
        provider_test_app(forward::ProviderMode::Gemini),
        Arc::new(AtomicBool::new(true)),
    )
    .oneshot(gemini_request)
    .await
    .unwrap();
    assert_eq!(gemini.status(), StatusCode::NOT_FOUND);
    let body = to_bytes(gemini.into_body(), 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["error"]["code"], 404);
    assert_eq!(body["error"]["status"], "NOT_FOUND");
}

#[tokio::test]
async fn exact_not_started_responses_increment_only_the_serving_plane_counter() {
    let app = provider_test_app(forward::ProviderMode::Anthropic);
    let metrics = Arc::clone(&app.metrics);
    forward::Metrics::inc(&metrics.positive_balance_402);
    let service = router(app, Arc::new(AtomicBool::new(true)));
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));

    let mut request = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("x-api-key", "admin-key")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(forward::is_exact_not_started_response(&response));
    assert_eq!(
        metrics.execution_not_started_count(forward::ProviderMode::Anthropic),
        1
    );
    assert_eq!(
        metrics.execution_not_started_count(forward::ProviderMode::OpenAi),
        0
    );
    assert_eq!(
        metrics.execution_not_started_count(forward::ProviderMode::Gemini),
        0
    );

    let mut request = Request::builder()
        .uri("/metrics")
        .header("x-api-key", "panel-key")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = String::from_utf8(
        to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(body.contains("claude_api_execution_not_started_total{plane=\"anthropic\"} 1"));
    assert!(body.contains("claude_api_execution_not_started_total{plane=\"openai\"} 0"));
    assert!(body.contains("claude_api_execution_not_started_total{plane=\"gemini\"} 0"));
    assert!(body.contains("claude_api_positive_balance_402_total 1"));
}

#[tokio::test]
async fn gemini_fleet_status_is_readonly_key_protected_and_runtime_scoped() {
    let service = router(
        provider_test_app(forward::ProviderMode::Gemini),
        Arc::new(AtomicBool::new(true)),
    );
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
    for (credential, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some("panel-key"), StatusCode::OK),
        (Some("control-key"), StatusCode::OK),
        (Some("admin-key"), StatusCode::OK),
    ] {
        let mut request = Request::builder()
            .uri("/gemini-subs")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        if let Some(key) = credential {
            request
                .headers_mut()
                .insert("x-api-key", key.parse().unwrap());
        }
        let response = service.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), expected, "credential {credential:?}");
        if expected == StatusCode::OK {
            let body = to_bytes(response.into_body(), 4_096).await.unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["enabled"], false);
            assert_eq!(body["profiles"], json!([]));
            let wire = body.to_string();
            for forbidden in [
                "subject",
                "email",
                "project_id",
                "refresh_token",
                "access_token",
                "client_secret",
                "proxy",
            ] {
                assert!(!wire.contains(forbidden), "leaked field {forbidden}");
            }
        }
    }
}

struct KimiHttpFixture {
    root: std::path::PathBuf,
}

impl KimiHttpFixture {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("kimi-subs-http-{}-{suffix}", std::process::id()));
        std::fs::create_dir_all(root.join("credentials")).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(
            root.join("credentials"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let fixture = Self { root };
        fixture.publish_console_profile();
        fixture
    }

    // Same sealed-envelope roster idiom as the forward gateway tests: tempdir 0700, keyring,
    // one console profile whose subject stays private to the gateway.
    fn publish_console_profile(&self) {
        use std::os::unix::fs::PermissionsExt;
        let credential = kimi_credential::KimiCredential {
            version: 1,
            kind: kimi_credential::KimiCredentialKind::ConsoleKey,
            access_token: "console-secret".into(),
            refresh_token: String::new(),
            expires_at: 0,
            scope: "coding".into(),
            subject_id: "subject-1".into(),
            plan_name: "unreviewed-base-plan".into(),
            plan_level: 1,
            status: kimi_credential::KIMI_STATUS_NORMAL.into(),
            region: "REGION_CN".into(),
            proxy_url: String::new(),
        };
        let ring =
            kimi_credential::CredentialKeyring::parse(&format!("a1:{}", "11".repeat(32))).unwrap();
        let envelope = ring.seal("a1", "kimi-01", &credential).unwrap();
        let credential_path = self.root.join("credentials/kimi-01.json");
        std::fs::write(
            &credential_path,
            kimi_credential::encode_envelope(&envelope).unwrap(),
        )
        .unwrap();
        std::fs::set_permissions(&credential_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let roster = json!({
            "profiles": [{
                "id": "kimi-01",
                "credential_file": credential_path.to_string_lossy(),
            }]
        });
        let roster_path = self.root.join("profiles.json");
        std::fs::write(&roster_path, serde_json::to_vec(&roster).unwrap()).unwrap();
        std::fs::set_permissions(&roster_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn gateway(&self) -> forward::KimiGateway {
        let config = forward::kimi::config::build(&forward::kimi::config::KimiPlaneInput {
            enabled: true,
            roster_dir: self.root.to_string_lossy().into_owned(),
            credential_keys: Some(format!("a1:{}", "11".repeat(32))),
            base_url: "https://127.0.0.1:9".into(),
            ..forward::kimi::config::KimiPlaneInput::default()
        })
        .unwrap()
        .unwrap();
        forward::KimiGateway::new_with_calibration(config, None).unwrap()
    }
}

impl Drop for KimiHttpFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn unknown_kimi_status() -> forward::KimiOperationalStatus {
    forward::KimiOperationalStatus {
        total_profiles: 1,
        live_profiles: 1,
        available_profiles: 1,
        auth_quarantined_profiles: 0,
        transport_cooling_profiles: 0,
        quota_cooling_profiles: 0,
        // The fixture profile carries the placeholder plan, so it is exactly the case this
        // counter exists to make visible.
        unreviewed_plan_profiles: 1,
        inflight_requests: 2,
        profiles: vec![forward::KimiProfileStatus {
            id: "kimi-01".to_string(),
            plan: "unreviewed",
            live: true,
            auth_quarantined_until: None,
            transport_cool_until: None,
            quota_cool_until: None,
            inflight: 2,
            quota_observed_at: Some(1_800_000_000),
            quota_windows: vec![forward::KimiQuotaWindowStatus {
                duration_secs: 18_000,
                used_units: 250,
                limit_units: 1_000,
                used_fraction_units: 25_000_000,
                measurement_resolution_fraction_units: 100_000,
                resets_at: 1_800_100_000,
                observed_at: 1_800_000_000,
            }],
        }],
        delivery: forward::kimi::queue::DeliveryHealth {
            pending_events: 1,
            dropped_events: 0,
            persistence_ok: true,
        },
    }
}

fn kimi_calibration_row(subject_id: &str) -> registry::KimiCalibrationRow {
    registry::KimiCalibrationRow {
        subject_id: subject_id.into(),
        plan: "unreviewed-base-plan".into(),
        window_duration_secs: 18_000,
        window_name: Some("rate".into()),
        resets_at: 1_800_100_000,
        anchor_used_fraction_units: 25_000_000,
        anchor_resolution_fraction_units: 100_000,
        anchor_spend_nano: 0,
        used_fraction_units: 25_000_000,
        measurement_resolution_fraction_units: 100_000,
        observed_at: 1_800_000_000,
        native_limit_units: 1_000,
        native_used_units: 250,
        observed_fraction_units: 0,
        observed_spend_nano: 0,
        samples: 0,
        unattributed_fraction_units: 0,
        current_capacity_nano: None,
        current_low_nano: None,
        current_high_nano: None,
        current_confidence_bp: 0,
        last_measured_at: None,
        estimator_version: 1,
        version: 1,
        updated_ts: 1_800_000_000,
    }
}

#[tokio::test]
async fn kimi_subs_is_control_key_protected_and_runtime_scoped() {
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
    for mode in [
        forward::ProviderMode::Anthropic,
        forward::ProviderMode::Combined,
    ] {
        let service = router(provider_test_app(mode), Arc::new(AtomicBool::new(true)));
        for (credential, expected) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some("panel-key"), StatusCode::UNAUTHORIZED),
            (Some("control-key"), StatusCode::OK),
            (Some("admin-key"), StatusCode::OK),
        ] {
            let mut request = Request::builder()
                .uri("/kimi-subs")
                .body(Body::empty())
                .unwrap();
            request.extensions_mut().insert(peer);
            if let Some(key) = credential {
                request
                    .headers_mut()
                    .insert("x-api-key", key.parse().unwrap());
            }
            let response = service.clone().oneshot(request).await.unwrap();
            assert_eq!(
                response.status(),
                expected,
                "{mode:?} credential {credential:?}"
            );
            if expected == StatusCode::OK {
                let body = to_bytes(response.into_body(), 4_096).await.unwrap();
                let body: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(body["enabled"], false);
                assert_eq!(body["profiles"], json!([]));
                assert!(body["now"].is_i64());
            }
        }
    }
}

#[tokio::test]
async fn kimi_subs_enabled_shape_is_redacted_and_bounded() {
    let fixture = KimiHttpFixture::new();
    let mut app = provider_test_app(forward::ProviderMode::Anthropic);
    app.kimi = Some(Arc::new(fixture.gateway()));
    let service = router(app, Arc::new(AtomicBool::new(true)));
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
    let mut request = Request::builder()
        .uri("/kimi-subs")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    request
        .headers_mut()
        .insert("x-api-key", "control-key".parse().unwrap());
    let response = service.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 65_536).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(body["enabled"], true);
    assert!(body["now"].is_i64());
    assert_eq!(body["delivery"]["pending_events"], 0);
    assert_eq!(body["delivery"]["dropped_events"], 0);
    assert_eq!(body["delivery"]["persistence_ok"], true);
    assert_eq!(body["fleet"]["profiles"], 1);
    assert_eq!(body["fleet"]["live_profiles"], 0);
    assert_eq!(body["fleet"]["available_profiles"], 1);
    assert_eq!(body["fleet"]["inflight_requests"], 0);
    let profile = &body["profiles"][0];
    assert_eq!(profile["id"], "kimi-01");
    // The provider-controlled plan string is bounded to the reviewed placeholder.
    assert_eq!(profile["plan"], "unreviewed");
    assert_eq!(profile["live"], false);
    assert!(profile["cooling"]["auth_until"].is_null());
    assert!(profile["cooling"]["transport_until"].is_null());
    assert!(profile["cooling"]["quota_until"].is_null());
    assert_eq!(profile["inflight"], 0);
    // Never observed stays null / empty, never zero or invented.
    assert!(profile["quota_observed_at"].is_null());
    assert_eq!(profile["quota"], json!([]));
    assert_eq!(profile["calibration"], json!([]));

    let wire = body.to_string();
    for forbidden in [
        "subject",
        "email",
        "phone",
        // Compound secret shapes (the Gemini scan idiom): bare "token" would false-positive on
        // the token-count and nano-per-token price vocabulary of conversion_models.
        "access_token",
        "refresh_token",
        "api_key",
        "authorization",
        "proxy",
        "credential",
        "request_id",
        "unreviewed-base-plan",
        "console-secret",
    ] {
        assert!(!wire.contains(forbidden), "leaked field {forbidden}");
    }
}

#[tokio::test]
async fn kimi_plane_serves_common_surface_and_kimi_subs_lattice() {
    let service = router(
        provider_test_app(forward::ProviderMode::Kimi),
        Arc::new(AtomicBool::new(true)),
    );
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));

    // The gateway is absent on the argv-pinned default-off plane: readiness stays green so
    // Caddy health-includes the slot serving the stable disabled envelope.
    let mut request = Request::builder()
        .uri("/ready")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 4_096).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    // The readiness probe itself is excluded from the gauge, so a healthy idle slot reads zero.
    assert_eq!(body, json!({"ready": true, "active_requests": 0}));

    // /metrics exports the label-free KIMI series as zero gauges on this plane as well.
    let mut request = Request::builder()
        .uri("/metrics")
        .header("x-api-key", "panel-key")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("claude_api_kimi_enabled 0"));
    assert!(body.contains("claude_api_kimi_live_profiles 0"));
    assert!(body.contains("claude_api_kimi_calibration_persistence_ok 0"));

    // /kimi-subs is registered on this plane with the same control lattice and answers the
    // disabled envelope while the gateway is absent.
    for (credential, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some("panel-key"), StatusCode::UNAUTHORIZED),
        (Some("control-key"), StatusCode::OK),
        (Some("admin-key"), StatusCode::OK),
    ] {
        let mut request = Request::builder()
            .uri("/kimi-subs")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        if let Some(key) = credential {
            request
                .headers_mut()
                .insert("x-api-key", key.parse().unwrap());
        }
        let response = service.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), expected, "credential {credential:?}");
        if expected == StatusCode::OK {
            let body = to_bytes(response.into_body(), 4_096).await.unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["enabled"], false);
            assert_eq!(body["profiles"], json!([]));
        }
    }
}

#[tokio::test]
async fn tripo3d_plane_serves_common_surface_and_disabled_envelope() {
    let service = router(
        provider_test_app(forward::ProviderMode::Tripo3d),
        Arc::new(AtomicBool::new(true)),
    );
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));

    // The gateway is absent on the argv-pinned default-off plane: readiness stays green so
    // the slot serving the stable disabled envelope is health-included.
    let mut request = Request::builder()
        .uri("/ready")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 4_096).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body, json!({"ready": true, "active_requests": 0}));

    // /metrics exports the label-free Tripo3D series as zero gauges on this plane as well.
    let mut request = Request::builder()
        .uri("/metrics")
        .header("x-api-key", "panel-key")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("claude_api_tripo3d_enabled 0"));
    assert!(body.contains("claude_api_tripo3d_live_profiles 0"));
    assert!(body.contains("claude_api_tripo3d_calibration_persistence_ok 0"));
    assert!(body.contains("claude_api_tripo3d_requests_total 0"));
    // Fixed cardinality: no profile/task/account labels anywhere in the plane's series.
    for line in body.lines().filter(|line| line.contains("tripo3d")) {
        assert!(
            !line.contains('{') || line.starts_with("# TYPE"),
            "labelled series: {line}"
        );
    }

    // /tripo3d-subs is registered on this plane with the same control lattice and answers the
    // disabled envelope while the gateway is absent.
    for (credential, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some("panel-key"), StatusCode::UNAUTHORIZED),
        (Some("control-key"), StatusCode::OK),
        (Some("admin-key"), StatusCode::OK),
    ] {
        let mut request = Request::builder()
            .uri("/tripo3d-subs")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        if let Some(key) = credential {
            request
                .headers_mut()
                .insert("x-api-key", key.parse().unwrap());
        }
        let response = service.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), expected, "credential {credential:?}");
        if expected == StatusCode::OK {
            let body = to_bytes(response.into_body(), 4_096).await.unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["enabled"], false);
            assert_eq!(body["profiles"], json!([]));
        }
    }

    // The plane surface 404s while the gateway is absent, and every unrouted path is the same
    // bounded 404 — nothing falls through to a Claude pool this process does not run.
    for (method, uri) in [
        ("POST", "/v1/3d/generations"),
        ("POST", "/v1/3d/uploads/image"),
        ("GET", "/v1/3d/tasks/00000000-0000-0000-0000-000000000000"),
        ("GET", "/v1/models"),
        ("POST", "/v1/messages"),
    ] {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("x-api-key", "admin-key")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        let response = service.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {uri}");
    }
}

#[tokio::test]
async fn tripo3d_plane_readiness_tracks_gateway_liveness() {
    // One assembled gateway with a cold roster (zero profiles) keeps the slot below its
    // readiness contract (live >= 1 and intact delivery persistence) → provider_unavailable.
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let root = std::env::temp_dir().join(format!("tripo3d-http-{suffix}"));
    std::fs::create_dir_all(root.join("artifacts")).unwrap();
    let config = forward::tripo3d::config::build(&forward::tripo3d::config::Tripo3dPlaneInput {
        enabled: true,
        roster_dir: root.to_string_lossy().into_owned(),
        credential_keys: Some(format!("a1:{}", "11".repeat(32))),
        credential_active_kid: None,
        balance_poll_secs: 300,
        artifact_dir: root.join("artifacts").to_string_lossy().into_owned(),
    })
    .unwrap()
    .unwrap();
    let gateway = forward::tripo3d::Tripo3dGateway::new_with_calibration(config, None).unwrap();

    let mut app = provider_test_app(forward::ProviderMode::Tripo3d);
    app.tripo3d = Some(gateway);
    let service = router(app, Arc::new(AtomicBool::new(true)));
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
    let mut request = Request::builder()
        .uri("/ready")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = to_bytes(response.into_body(), 4_096).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        body,
        json!({"ready": false, "reason": "provider_unavailable", "active_requests": 0})
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn suno_plane_serves_common_surface_and_disabled_envelope() {
    let service = router(
        provider_test_app(forward::ProviderMode::Suno),
        Arc::new(AtomicBool::new(true)),
    );
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));

    // The gateway is absent on the argv-pinned default-off plane: readiness stays green so
    // the slot serving the stable disabled envelope is health-included.
    let mut request = Request::builder()
        .uri("/ready")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 4_096).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body, json!({"ready": true, "active_requests": 0}));

    // /metrics exports the label-free Suno series as zero gauges on this plane as well.
    let mut request = Request::builder()
        .uri("/metrics")
        .header("x-api-key", "panel-key")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("claude_api_suno_enabled 0"));
    assert!(body.contains("claude_api_suno_live_profiles 0"));
    assert!(body.contains("claude_api_suno_calibration_persistence_ok 0"));
    assert!(body.contains("claude_api_suno_requests_total 0"));

    // /suno-subs is registered on this plane with the same control lattice and answers the
    // disabled envelope while the gateway is absent.
    for (credential, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some("panel-key"), StatusCode::UNAUTHORIZED),
        (Some("control-key"), StatusCode::OK),
        (Some("admin-key"), StatusCode::OK),
    ] {
        let mut request = Request::builder()
            .uri("/suno-subs")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        if let Some(key) = credential {
            request
                .headers_mut()
                .insert("x-api-key", key.parse().unwrap());
        }
        let response = service.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), expected, "credential {credential:?}");
        if expected == StatusCode::OK {
            let body = to_bytes(response.into_body(), 4_096).await.unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["enabled"], false);
            assert_eq!(body["profiles"], json!([]));
        }
    }

    // The audio surface exists but fails closed without the gateway: generation creation is a
    // bounded 404 (no Claude pool to fall into), and every unrouted path is the same 404.
    for (method, uri) in [
        (Method::POST, "/v1/audio/generations"),
        (Method::POST, "/v1/audio/uploads"),
        (Method::GET, "/v1/audio/generations/gen-1"),
        (
            Method::GET,
            "/v1/audio/generations/gen-1/artifact/audio_url.mp3",
        ),
        (Method::POST, "/v1/messages"),
    ] {
        let mut request = Request::builder()
            .method(method.clone())
            .uri(uri)
            .header("x-api-key", "admin-key")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        let response = service.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {uri}");
    }
}

#[tokio::test]
async fn suno_plane_readiness_tracks_gateway_liveness() {
    // One assembled gateway with a cold roster (zero profiles) keeps the slot below its
    // readiness contract (live >= 1 and intact delivery persistence) → provider_unavailable.
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let root = std::env::temp_dir().join(format!("suno-http-{suffix}"));
    std::fs::create_dir_all(root.join("artifacts")).unwrap();
    let config = forward::suno::config::build(&forward::suno::config::SunoPlaneInput {
        enabled: true,
        roster_dir: root.to_string_lossy().into_owned(),
        credential_keys: Some(format!("a1:{}", "11".repeat(32))),
        credential_active_kid: None,
        quota_poll_secs: 300,
        artifact_dir: root.join("artifacts").to_string_lossy().into_owned(),
    })
    .unwrap()
    .unwrap();
    let gateway = forward::suno::SunoGateway::new_with_calibration(config, None).unwrap();

    let mut app = provider_test_app(forward::ProviderMode::Suno);
    app.suno = Some(gateway);
    let service = router(app, Arc::new(AtomicBool::new(true)));
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
    let mut request = Request::builder()
        .uri("/ready")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = to_bytes(response.into_body(), 4_096).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        body,
        json!({"ready": false, "reason": "provider_unavailable", "active_requests": 0})
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn kimi_plane_readiness_tracks_gateway_liveness() {
    // One rostered profile that never authenticated keeps the gateway below its readiness
    // contract (live >= 1 and intact delivery persistence), mapped to provider_unavailable.
    let fixture = KimiHttpFixture::new();
    let mut app = provider_test_app(forward::ProviderMode::Kimi);
    app.kimi = Some(Arc::new(fixture.gateway()));
    let service = router(app, Arc::new(AtomicBool::new(true)));
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
    let mut request = Request::builder()
        .uri("/ready")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = to_bytes(response.into_body(), 4_096).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        body,
        json!({"ready": false, "reason": "provider_unavailable", "active_requests": 0})
    );
}

#[tokio::test]
async fn kimi_plane_messages_fails_closed_for_non_kimi_models() {
    let service = router(
        provider_test_app(forward::ProviderMode::Kimi),
        Arc::new(AtomicBool::new(true)),
    );
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));

    // A Claude model on this plane must never reach a Claude pool: bounded static 404.
    let mut request = Request::builder()
        .method(Method::POST)
        .uri("/v1/messages")
        .header("x-api-key", "admin-key")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"model":"claude-sonnet-5","max_tokens":1,"messages":[]}"#,
        ))
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = to_bytes(response.into_body(), 4_096).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        body,
        json!({"type": "error", "error": {"type": "not_found_error", "message": "Not Found"}})
    );
    let wire = body.to_string();
    for forbidden in ["kimi", "subscription", "pool", "upstream"] {
        assert!(!wire.contains(forbidden), "leaked {forbidden}");
    }

    // An exact KIMI alias with no composed gateway fails closed through the same dispatch the
    // Anthropic path uses — never a fallthrough into another provider.
    let mut request = Request::builder()
        .method(Method::POST)
        .uri("/v1/messages")
        .header("x-api-key", "admin-key")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"model":"kimi-for-coding","max_tokens":1,"messages":[]}"#,
        ))
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::from_u16(529).unwrap());
    let body = to_bytes(response.into_body(), 4_096).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["error"]["type"], "overloaded_error");

    // Other provider planes' routes stay unregistered here and fail closed as well.
    for path in ["/v1/chat/completions", "/v1/responses"] {
        let mut request = Request::builder()
            .method(Method::POST)
            .uri(path)
            .header("x-api-key", "admin-key")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        request.extensions_mut().insert(peer);
        let response = service.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[test]
fn kimi_subs_value_serializes_the_exact_quota_window() {
    let fixture = KimiHttpFixture::new();
    let gateway = fixture.gateway();
    let value =
        kimi_subs_value_with_report(&gateway, &unknown_kimi_status(), 1_800_000_100, None, None);
    assert_eq!(value["fleet"]["inflight_requests"], 2);
    assert_eq!(value["delivery"]["pending_events"], 1);
    // The runner attribution surface: authority flag, bounded recent-turn list and the
    // official rate card for worst-case bounds.
    assert_eq!(value["calibration_authority_available"], false);
    assert_eq!(
        value["calibration_recent_turn_limit"],
        registry::MAX_RECENT_PROVIDER_TURN_CALIBRATION_EVENTS
    );
    assert_eq!(value["calibration_recent_turns"], json!([]));
    let models = value["conversion_models"].as_array().unwrap();
    assert!(!models.is_empty());
    assert!(models.iter().any(|model| model["id"] == "kimi-k2.7-code"
        && model["api"]["output_nano_per_token"] == "4000"
        && model["api_tariff_schedule_id"] == metering::kimi::KIMI_TARIFF_SCHEDULE_ID));
    assert_eq!(value["profiles"][0]["quota_observed_at"], 1_800_000_000);
    let window = &value["profiles"][0]["quota"][0];
    assert_eq!(window["duration_secs"], 18_000);
    assert_eq!(window["used_units"], 250);
    assert_eq!(window["limit_units"], 1_000);
    // Exact fraction semantics: 25% in 10^-8 units, real resolution of a limit-1000 counter.
    assert_eq!(window["used_fraction_units"], 25_000_000);
    assert_eq!(window["measurement_resolution_fraction_units"], 100_000);
    assert_eq!(window["resets_at"], 1_800_100_000);
    assert_eq!(window["observed_at"], 1_800_000_000);
    assert_eq!(value["profiles"][0]["calibration"], json!([]));
}

#[test]
fn kimi_subs_value_joins_calibration_through_the_opaque_id_and_drops_unknown_subjects() {
    let fixture = KimiHttpFixture::new();
    let gateway = fixture.gateway();
    let mut measured = kimi_calibration_row("subject-1");
    measured.samples = 2;
    measured.observed_spend_nano = 1_250_000_000;
    measured.current_capacity_nano = Some(50_000_000_000);
    measured.current_low_nano = Some(40_000_000_000);
    measured.current_confidence_bp = 9_000;
    measured.last_measured_at = Some(1_800_000_000);
    let unknown = kimi_calibration_row("subject-unknown");
    let report = vec![measured, unknown];

    let value = kimi_subs_value_with_report(
        &gateway,
        &unknown_kimi_status(),
        1_800_000_100,
        Some(&report),
        None,
    );
    let calibration = &value["profiles"][0]["calibration"];
    // The roster-less subject stays durable for audit but is never serialized.
    assert_eq!(calibration.as_array().unwrap().len(), 1);
    let entry = &calibration[0];
    assert_eq!(entry["duration_secs"], 18_000);
    assert_eq!(entry["samples"], 2);
    assert_eq!(entry["confidence_bp"], 9_000);
    // Money integers are decimal strings (BigInt-safe); unknown high stays null.
    assert_eq!(entry["capacity"]["current_nano"], "50000000000");
    assert_eq!(entry["capacity"]["low_nano"], "40000000000");
    assert!(entry["capacity"]["high_nano"].is_null());
    assert_eq!(entry["remaining"]["native_units"], 750);
    assert_eq!(entry["remaining"]["api_nano"], "37500000000");
    assert_eq!(entry["observed_spend_nano"], "1250000000");
    assert_eq!(entry["last_measured_at"], 1_800_000_000);
    assert_eq!(entry["estimator_version"], 1);

    let wire = value.to_string();
    for forbidden in ["subject", "unreviewed-base-plan"] {
        assert!(!wire.contains(forbidden), "leaked field {forbidden}");
    }
}

#[test]
fn kimi_subs_value_publishes_only_the_current_plan_cohort() {
    let fixture = KimiHttpFixture::new();
    let gateway = fixture.gateway();
    let mut current = kimi_calibration_row("subject-1");
    current.samples = 2;
    let mut stale = kimi_calibration_row("subject-1");
    stale.plan = "Vivace".into();
    stale.samples = 99;
    let report = vec![stale, current];
    let value = kimi_subs_value_with_report(
        &gateway,
        &unknown_kimi_status(),
        1_800_000_100,
        Some(&report),
        None,
    );
    let calibration = &value["profiles"][0]["calibration"];
    // Both rows share the subject and the window, but a plan change made "Vivace" a different
    // cohort: only the profile's current plan is its money. The stale cohort stays durable for
    // audit and is never published — without this filter the admin picked whichever row sorted
    // first for the window.
    assert_eq!(calibration.as_array().unwrap().len(), 1);
    assert_eq!(calibration[0]["samples"], 2);
}

#[test]
fn kimi_subs_value_keeps_unknown_capacity_and_remaining_null_never_zero() {
    let fixture = KimiHttpFixture::new();
    let gateway = fixture.gateway();
    let report = vec![kimi_calibration_row("subject-1")];
    let value = kimi_subs_value_with_report(
        &gateway,
        &unknown_kimi_status(),
        1_800_000_100,
        Some(&report),
        None,
    );
    let entry = &value["profiles"][0]["calibration"][0];
    assert!(entry["capacity"]["current_nano"].is_null());
    assert!(entry["capacity"]["low_nano"].is_null());
    assert!(entry["capacity"]["high_nano"].is_null());
    // Native remaining needs no estimation; the API-dollar one stays null while capacity is
    // unknown — never a zero or an invented nominal.
    assert_eq!(entry["remaining"]["native_units"], 750);
    assert!(entry["remaining"]["api_nano"].is_null());
    assert_eq!(entry["observed_spend_nano"], "0");
    assert_eq!(entry["samples"], 0);
    assert!(entry["last_measured_at"].is_null());

    // An overflowing row (used so negative the remainder cannot be represented) has no native
    // remainder either: the whole object is null rather than a negative or zero figure.
    let mut malformed = kimi_calibration_row("subject-1");
    malformed.native_used_units = i64::MIN;
    let report = vec![malformed];
    let value = kimi_subs_value_with_report(
        &gateway,
        &unknown_kimi_status(),
        1_800_000_100,
        Some(&report),
        None,
    );
    assert!(value["profiles"][0]["calibration"][0]["remaining"].is_null());
}

#[test]
fn kimi_subs_recent_turns_join_through_the_opaque_id_and_bound_the_plan() {
    let fixture = KimiHttpFixture::new();
    let gateway = fixture.gateway();
    let event = registry::KimiTurnCalibrationEvent {
        request_id: "123e4567-e89b-42d3-a456-426614174000".into(),
        subject_id: "subject-1".into(),
        plan: "unreviewed-base-plan".into(),
        requested_model: "kimi-for-coding".into(),
        served_model: "kimi-k2.7-code".into(),
        context_mode: "256k".into(),
        reasoning_effort: "high".into(),
        tariff_schedule_id: metering::kimi::KIMI_TARIFF_SCHEDULE_ID.into(),
        priced_ts: 1_800_000_000,
        completed_at: 1_800_000_001,
        input_tokens: 10,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        output_tokens: 2,
        reasoning_output_tokens: 0,
        api_input_nanousd: 600_000,
        api_cache_read_nanousd: 0,
        api_cache_write_nanousd: 0,
        api_output_nanousd: 600_000,
        api_total_nanousd: 1_200_000,
    };
    let mut foreign = event.clone();
    foreign.subject_id = "subject-unknown".into();
    foreign.request_id = "223e4567-e89b-42d3-a456-426614174000".into();
    let turns = vec![event, foreign];

    let value = kimi_subs_value_with_report(
        &gateway,
        &unknown_kimi_status(),
        1_800_000_100,
        None,
        Some(&turns),
    );
    let recent = value["calibration_recent_turns"].as_array().unwrap();
    // The roster-less subject's turn stays durable but is never serialized.
    assert_eq!(recent.len(), 1);
    let turn = &recent[0];
    assert_eq!(turn["request_id"], "123e4567-e89b-42d3-a456-426614174000");
    assert_eq!(turn["profile_id"], "kimi-01");
    // The provider-controlled plan string is bounded to the reviewed placeholder.
    assert_eq!(turn["plan"], "unreviewed");
    assert_eq!(turn["served_model"], "kimi-k2.7-code");
    assert_eq!(turn["api_total_nanousd"], "1200000");

    let wire = value.to_string();
    for forbidden in ["subject", "unreviewed-base-plan"] {
        assert!(!wire.contains(forbidden), "leaked field {forbidden}");
    }
}

#[test]
fn prometheus_kimi_series_are_zero_gauges_without_a_plane_and_never_labelled() {
    let mut body = String::new();
    write_kimi_operational_metrics(&mut body, false, None, &forward::Metrics::default());
    for sample in [
        "claude_api_kimi_enabled 0",
        "claude_api_kimi_profiles 0",
        "claude_api_kimi_live_profiles 0",
        "claude_api_kimi_available_profiles 0",
        "claude_api_kimi_inflight_requests 0",
        "claude_api_kimi_auth_quarantined_profiles 0",
        "claude_api_kimi_transport_cooling_profiles 0",
        "claude_api_kimi_quota_cooling_profiles 0",
        "claude_api_kimi_calibration_pending_events 0",
        "claude_api_kimi_calibration_dropped_events_total 0",
        "claude_api_kimi_calibration_persistence_ok 0",
    ] {
        assert!(body.contains(sample), "missing {sample}");
    }
    // Never observed: the timestamp series is omitted entirely rather than emitted as 0.
    assert!(!body.contains("claude_api_kimi_quota_last_observation_timestamp_seconds"));
    for line in body
        .lines()
        .filter(|line| line.starts_with("claude_api_kimi") && !line.starts_with('#'))
    {
        assert!(
            !line.contains('{'),
            "kimi series must carry no labels at all: {line}"
        );
    }
}

#[test]
fn prometheus_kimi_series_report_fleet_aggregates_and_the_freshest_observation() {
    let status = unknown_kimi_status();
    let mut body = String::new();
    write_kimi_operational_metrics(&mut body, true, Some(&status), &forward::Metrics::default());
    for sample in [
        "claude_api_kimi_enabled 1",
        "claude_api_kimi_profiles 1",
        "claude_api_kimi_live_profiles 1",
        "claude_api_kimi_available_profiles 1",
        "claude_api_kimi_inflight_requests 2",
        "claude_api_kimi_auth_quarantined_profiles 0",
        "claude_api_kimi_transport_cooling_profiles 0",
        "claude_api_kimi_quota_cooling_profiles 0",
        "claude_api_kimi_calibration_pending_events 1",
        "claude_api_kimi_calibration_dropped_events_total 0",
        "claude_api_kimi_calibration_persistence_ok 1",
        "claude_api_kimi_quota_last_observation_timestamp_seconds 1800000000",
    ] {
        assert!(body.contains(sample), "missing {sample}");
    }
    for line in body
        .lines()
        .filter(|line| line.starts_with("claude_api_kimi") && !line.starts_with('#'))
    {
        assert!(
            !line.contains('{'),
            "kimi series must carry no labels at all: {line}"
        );
    }
}

#[tokio::test]
async fn metrics_endpoint_publishes_label_free_kimi_zero_gauges_for_a_disabled_plane() {
    let service = router(admin_auth_test_app(), Arc::new(AtomicBool::new(true)));
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
    let mut request = Request::builder()
        .uri("/metrics")
        .header("x-api-key", "panel-key")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = String::from_utf8(
        to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(body.contains("claude_api_kimi_enabled 0"));
    assert!(body.contains("claude_api_kimi_profiles 0"));
    assert!(!body.contains("claude_api_kimi_quota_last_observation_timestamp_seconds"));
    for line in body
        .lines()
        .filter(|line| line.starts_with("claude_api_kimi") && !line.starts_with('#'))
    {
        assert!(
            !line.contains('{'),
            "kimi series must carry no labels at all: {line}"
        );
    }
}

struct GlmHttpFixture {
    root: std::path::PathBuf,
}

impl GlmHttpFixture {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("glm-subs-http-{}-{suffix}", std::process::id()));
        std::fs::create_dir_all(root.join("credentials")).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(
            root.join("credentials"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let fixture = Self { root };
        fixture.publish_profiles(&[("glm-01", "zai-test-key-1")]);
        fixture
    }

    // Same sealed-envelope roster idiom as the forward gateway tests: tempdir 0700, keyring,
    // per-profile plan keys whose subject digests stay private to the gateway.
    fn publish_profiles(&self, profiles: &[(&str, &str)]) {
        use std::os::unix::fs::PermissionsExt;
        let ring =
            glm_credential::CredentialKeyring::parse(&format!("a1:{}", "11".repeat(32))).unwrap();
        let mut entries = Vec::with_capacity(profiles.len());
        for (id, api_key) in profiles {
            let credential = glm_credential::GlmCredential {
                version: 1,
                kind: glm_credential::GlmCredentialKind::PlanKey,
                api_key: (*api_key).into(),
                plan: glm_credential::GlmPlan::Pro,
                base_url: glm_credential::GLM_BASE_URL_INTERNATIONAL.into(),
                proxy_url: String::new(),
            };
            let envelope = ring.seal("a1", id, &credential).unwrap();
            let credential_path = self.root.join("credentials").join(format!("{id}.json"));
            std::fs::write(
                &credential_path,
                glm_credential::encode_envelope(&envelope).unwrap(),
            )
            .unwrap();
            std::fs::set_permissions(&credential_path, std::fs::Permissions::from_mode(0o600))
                .unwrap();
            entries.push(json!({
                "id": id,
                "credential_file": credential_path.to_string_lossy(),
            }));
        }
        let roster = json!({ "profiles": entries });
        let roster_path = self.root.join("profiles.json");
        std::fs::write(&roster_path, serde_json::to_vec(&roster).unwrap()).unwrap();
        std::fs::set_permissions(&roster_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn gateway(&self) -> forward::glm::GlmGateway {
        let config = forward::glm::config::build(&forward::glm::config::GlmPlaneInput {
            enabled: true,
            roster_dir: self.root.to_string_lossy().into_owned(),
            credential_keys: Some(format!("a1:{}", "11".repeat(32))),
            auth_scheme: "bearer".into(),
            quota_poll_secs: 300,
        })
        .unwrap()
        .unwrap();
        forward::glm::GlmGateway::new_with_calibration(config, None).unwrap()
    }
}

impl Drop for GlmHttpFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn unknown_glm_status() -> forward::glm::GlmOperationalStatus {
    forward::glm::GlmOperationalStatus {
        total_profiles: 1,
        live_profiles: 1,
        available_profiles: 1,
        account_dead_profiles: 0,
        account_suspect_profiles: 0,
        transport_cooling_profiles: 0,
        quota_cooling_profiles: 0,
        inflight_requests: 2,
        missing_terminal_usage: 3,
        served_model_rejected: 2,
        profiles: vec![forward::glm::GlmProfileStatus {
            id: "glm-01".to_string(),
            plan: "Pro",
            live: true,
            account_dead: false,
            account_suspect: false,
            transport_cool_until: None,
            quota_cool_until: None,
            inflight: 2,
            quota_observed_at: Some(1_800_000_000),
            quota_windows: vec![forward::glm::GlmQuotaWindowStatus {
                duration_secs: 18_000,
                used_units: Some(250),
                limit_units: Some(1_000),
                remaining_units: Some(750),
                used_fraction_units: Some(25_000_000),
                measurement_resolution_fraction_units: Some(100_000),
                resets_at: Some(1_800_100_000),
                observed_at: 1_800_000_000,
            }],
        }],
        delivery: forward::glm::queue::DeliveryHealth {
            pending_events: 1,
            dropped_events: 0,
            persistence_ok: true,
        },
    }
}

fn glm_calibration_row(subject_id: &str) -> registry::GlmCalibrationRow {
    registry::GlmCalibrationRow {
        subject_id: subject_id.into(),
        plan: "Pro".into(),
        window_duration_secs: registry::GLM_5H_WINDOW_SECS,
        reset_at: Some(1_800_100_000),
        anchor_used_fraction_units: Some(25_000_000),
        anchor_resolution_fraction_units: Some(100_000),
        anchor_spend_api_nanousd: 0,
        anchor_spend_native_microcredits: 0,
        used_fraction_units: Some(25_000_000),
        measurement_resolution_fraction_units: Some(100_000),
        observed_at: 1_800_000_000,
        native_limit_microcredits: Some(1_000),
        native_used_microcredits: Some(250),
        observed_fraction_units: 0,
        observed_spend_api_nanousd: 0,
        observed_spend_native_microcredits: 0,
        samples: 0,
        unattributed_fraction_units: 0,
        current_capacity_nanousd: None,
        current_low_nanousd: None,
        current_high_nanousd: None,
        current_confidence_bp: 0,
        last_measured_at: None,
        estimator_version: 1,
        version: 1,
        updated_ts: 1_800_000_000,
    }
}

#[tokio::test]
async fn glm_subs_is_control_key_protected_and_runtime_scoped() {
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
    for mode in [
        forward::ProviderMode::Anthropic,
        forward::ProviderMode::Combined,
    ] {
        let service = router(provider_test_app(mode), Arc::new(AtomicBool::new(true)));
        for (credential, expected) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some("panel-key"), StatusCode::UNAUTHORIZED),
            (Some("control-key"), StatusCode::OK),
            (Some("admin-key"), StatusCode::OK),
        ] {
            let mut request = Request::builder()
                .uri("/glm-subs")
                .body(Body::empty())
                .unwrap();
            request.extensions_mut().insert(peer);
            if let Some(key) = credential {
                request
                    .headers_mut()
                    .insert("x-api-key", key.parse().unwrap());
            }
            let response = service.clone().oneshot(request).await.unwrap();
            assert_eq!(
                response.status(),
                expected,
                "{mode:?} credential {credential:?}"
            );
            if expected == StatusCode::OK {
                let body = to_bytes(response.into_body(), 4_096).await.unwrap();
                let body: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(body["enabled"], false);
                assert_eq!(body["profiles"], json!([]));
                assert!(body["now"].is_i64());
            }
        }
    }
}

#[tokio::test]
async fn glm_subs_enabled_shape_is_redacted_and_bounded() {
    let fixture = GlmHttpFixture::new();
    let mut app = provider_test_app(forward::ProviderMode::Anthropic);
    app.glm = Some(Arc::new(fixture.gateway()));
    let service = router(app, Arc::new(AtomicBool::new(true)));
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
    let mut request = Request::builder()
        .uri("/glm-subs")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    request
        .headers_mut()
        .insert("x-api-key", "control-key".parse().unwrap());
    let response = service.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 65_536).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(body["enabled"], true);
    assert!(body["now"].is_i64());
    assert_eq!(body["delivery"]["pending_events"], 0);
    assert_eq!(body["delivery"]["dropped_events"], 0);
    assert_eq!(body["delivery"]["persistence_ok"], true);
    assert_eq!(body["fleet"]["profiles"], 1);
    assert_eq!(body["fleet"]["live_profiles"], 0);
    assert_eq!(body["fleet"]["available_profiles"], 1);
    assert_eq!(body["fleet"]["inflight_requests"], 0);
    assert_eq!(body["fleet"]["account_dead_profiles"], 0);
    assert_eq!(body["fleet"]["account_suspect_profiles"], 0);
    // No durable rows yet: both canonical windows stay entirely unknown, never zero-filled.
    let totals = body["window_totals"].as_array().unwrap();
    assert_eq!(totals.len(), 2);
    assert_eq!(totals[0]["window_minutes"], 300);
    assert_eq!(totals[0]["duration_secs"], 18_000);
    assert!(totals[0]["capacity_nano"].is_null());
    assert!(totals[0]["remaining_nano"].is_null());
    assert_eq!(totals[1]["window_minutes"], 10_080);
    assert_eq!(totals[1]["duration_secs"], 604_800);
    assert!(totals[1]["capacity_nano"].is_null());
    assert!(totals[1]["remaining_nano"].is_null());
    let profile = &body["profiles"][0];
    assert_eq!(profile["id"], "glm-01");
    // The roster constrains GLM to the three reviewed individual plans, so the exact
    // declared plan is the bounded label.
    assert_eq!(profile["plan"], "Pro");
    assert_eq!(profile["live"], false);
    assert_eq!(profile["account_dead"], false);
    assert_eq!(profile["account_suspect"], false);
    assert!(profile["cooling"]["transport_until"].is_null());
    assert!(profile["cooling"]["quota_until"].is_null());
    assert_eq!(profile["inflight"], 0);
    // Never observed stays null / empty, never zero or invented.
    assert!(profile["quota_observed_at"].is_null());
    assert_eq!(profile["quota"], json!([]));
    assert_eq!(profile["calibration"], json!([]));

    let wire = body.to_string();
    for forbidden in [
        "subject",
        "email",
        "phone",
        "token",
        "proxy",
        "credential",
        "request_id",
        "api_key",
        "base_url",
        "zai-test-key-1",
        "api.z.ai",
    ] {
        assert!(!wire.contains(forbidden), "leaked field {forbidden}");
    }
}

#[test]
fn glm_subs_value_serializes_the_exact_quota_window() {
    let fixture = GlmHttpFixture::new();
    let gateway = fixture.gateway();
    let value = glm_subs_value_with_report(&gateway, &unknown_glm_status(), 1_800_000_100, None);
    assert_eq!(value["fleet"]["inflight_requests"], 2);
    assert_eq!(value["delivery"]["pending_events"], 1);
    assert_eq!(value["profiles"][0]["quota_observed_at"], 1_800_000_000);
    let window = &value["profiles"][0]["quota"][0];
    assert_eq!(window["duration_secs"], 18_000);
    assert_eq!(window["used_units"], 250);
    assert_eq!(window["limit_units"], 1_000);
    assert_eq!(window["remaining_units"], 750);
    // Exact fraction semantics: 25% in 10^-8 units, real resolution of a limit-1000 counter.
    assert_eq!(window["used_fraction_units"], 25_000_000);
    assert_eq!(window["measurement_resolution_fraction_units"], 100_000);
    assert_eq!(window["resets_at"], 1_800_100_000);
    assert_eq!(window["observed_at"], 1_800_000_000);
    assert_eq!(value["profiles"][0]["calibration"], json!([]));
}

#[test]
fn glm_subs_value_joins_calibration_through_the_opaque_id_and_drops_unknown_subjects() {
    let fixture = GlmHttpFixture::new();
    let gateway = fixture.gateway();
    let subject = forward::glm::roster::subject_id_of("zai-test-key-1");
    let mut measured = glm_calibration_row(&subject);
    measured.samples = 2;
    measured.observed_spend_api_nanousd = 1_250_000_000;
    measured.observed_spend_native_microcredits = 500_000_000;
    measured.current_capacity_nanousd = Some(50_000_000_000);
    measured.current_low_nanousd = Some(40_000_000_000);
    measured.current_confidence_bp = 9_000;
    measured.last_measured_at = Some(1_800_000_000);
    let unknown = glm_calibration_row("glm-subject-unknown");
    let report = vec![measured, unknown];

    let value = glm_subs_value_with_report(
        &gateway,
        &unknown_glm_status(),
        1_800_000_100,
        Some(&report),
    );
    let calibration = &value["profiles"][0]["calibration"];
    // The roster-less subject stays durable for audit but is never serialized.
    assert_eq!(calibration.as_array().unwrap().len(), 1);
    let entry = &calibration[0];
    assert_eq!(entry["duration_secs"], 18_000);
    assert_eq!(entry["samples"], 2);
    assert_eq!(entry["confidence_bp"], 9_000);
    // Money integers are decimal strings (BigInt-safe); unknown high stays null.
    assert_eq!(entry["capacity"]["current_nano"], "50000000000");
    assert_eq!(entry["capacity"]["low_nano"], "40000000000");
    assert!(entry["capacity"]["high_nano"].is_null());
    assert_eq!(entry["remaining"]["native_units"], 750);
    assert_eq!(entry["remaining"]["api_nano"], "37500000000");
    assert_eq!(entry["observed_spend_nano"], "1250000000");
    assert_eq!(entry["observed_spend_native_units"], 500_000_000);
    assert_eq!(entry["last_measured_at"], 1_800_000_000);
    assert_eq!(entry["estimator_version"], 1);

    let wire = value.to_string();
    assert!(!wire.contains("subject"), "leaked subject digest");
    assert!(!wire.contains(&subject), "leaked subject digest value");
}

#[test]
fn glm_subs_value_publishes_only_the_current_plan_cohort() {
    let fixture = GlmHttpFixture::new();
    let gateway = fixture.gateway();
    let subject = forward::glm::roster::subject_id_of("zai-test-key-1");
    let mut current = glm_calibration_row(&subject);
    current.samples = 2;
    let mut stale = glm_calibration_row(&subject);
    stale.plan = "Max".into();
    stale.samples = 99;
    let report = vec![stale, current];
    let value = glm_subs_value_with_report(
        &gateway,
        &unknown_glm_status(),
        1_800_000_100,
        Some(&report),
    );
    let calibration = &value["profiles"][0]["calibration"];
    // Both rows share the subject and the window, but a plan change made "Max" a different
    // cohort: only the profile's current plan is its money. The stale cohort stays durable for
    // audit and is never published.
    assert_eq!(calibration.as_array().unwrap().len(), 1);
    assert_eq!(calibration[0]["samples"], 2);
}

#[test]
fn glm_subs_value_keeps_unknown_capacity_and_remaining_null_never_zero() {
    let fixture = GlmHttpFixture::new();
    let gateway = fixture.gateway();
    let subject = forward::glm::roster::subject_id_of("zai-test-key-1");
    let report = vec![glm_calibration_row(&subject)];
    let value = glm_subs_value_with_report(
        &gateway,
        &unknown_glm_status(),
        1_800_000_100,
        Some(&report),
    );
    let entry = &value["profiles"][0]["calibration"][0];
    assert!(entry["capacity"]["current_nano"].is_null());
    assert!(entry["capacity"]["low_nano"].is_null());
    assert!(entry["capacity"]["high_nano"].is_null());
    // Native remaining needs no estimation; the API-dollar one stays null while capacity is
    // unknown — never a zero or an invented nominal.
    assert_eq!(entry["remaining"]["native_units"], 750);
    assert!(entry["remaining"]["api_nano"].is_null());
    assert_eq!(entry["observed_spend_nano"], "0");
    assert_eq!(entry["samples"], 0);
    assert!(entry["last_measured_at"].is_null());

    // An overflowing row (used so negative the remainder cannot be represented) has no native
    // remainder either: the whole object is null rather than a negative or zero figure.
    let mut malformed = glm_calibration_row(&subject);
    malformed.native_used_microcredits = Some(i64::MIN);
    let report = vec![malformed];
    let value = glm_subs_value_with_report(
        &gateway,
        &unknown_glm_status(),
        1_800_000_100,
        Some(&report),
    );
    assert!(value["profiles"][0]["calibration"][0]["remaining"].is_null());
}

#[test]
fn glm_subs_window_totals_sum_known_rows_and_keep_unknown_null() {
    let fixture = GlmHttpFixture::new();
    fixture.publish_profiles(&[("glm-01", "zai-test-key-1"), ("glm-02", "zai-test-key-2")]);
    let gateway = fixture.gateway();
    let mut status = unknown_glm_status();
    status.total_profiles = 2;
    status.profiles.push(forward::glm::GlmProfileStatus {
        id: "glm-02".to_string(),
        plan: "Pro",
        live: true,
        account_dead: false,
        account_suspect: false,
        transport_cool_until: None,
        quota_cool_until: None,
        inflight: 0,
        quota_observed_at: None,
        quota_windows: Vec::new(),
    });
    let subject_one = forward::glm::roster::subject_id_of("zai-test-key-1");
    let subject_two = forward::glm::roster::subject_id_of("zai-test-key-2");
    let measured = |subject: &str, duration_secs: i64, capacity: i64, used_fraction: i64| {
        let mut row = glm_calibration_row(subject);
        row.window_duration_secs = duration_secs;
        row.used_fraction_units = Some(used_fraction);
        row.current_capacity_nanousd = Some(capacity);
        row
    };
    let report = vec![
        measured(
            &subject_one,
            registry::GLM_5H_WINDOW_SECS,
            50_000_000_000,
            25_000_000,
        ),
        measured(
            &subject_one,
            registry::GLM_WEEKLY_WINDOW_SECS,
            200_000_000_000,
            10_000_000,
        ),
        measured(
            &subject_two,
            registry::GLM_5H_WINDOW_SECS,
            30_000_000_000,
            50_000_000,
        ),
        measured(
            &subject_two,
            registry::GLM_WEEKLY_WINDOW_SECS,
            100_000_000_000,
            0,
        ),
    ];
    let value = glm_subs_value_with_report(&gateway, &status, 1_800_000_100, Some(&report));
    let totals = value["window_totals"].as_array().unwrap();
    assert_eq!(totals.len(), 2);
    // 5h window: capacity 50e9 + 30e9, remaining 37.5e9 (75% of 50e9) + 15e9 (50% of 30e9).
    assert_eq!(totals[0]["window_minutes"], 300);
    assert_eq!(totals[0]["duration_secs"], 18_000);
    assert_eq!(totals[0]["capacity_nano"], "80000000000");
    assert_eq!(totals[0]["remaining_nano"], "52500000000");
    // Weekly window: capacity 200e9 + 100e9, remaining 180e9 (90%) + 100e9 (100%).
    assert_eq!(totals[1]["window_minutes"], 10_080);
    assert_eq!(totals[1]["duration_secs"], 604_800);
    assert_eq!(totals[1]["capacity_nano"], "300000000000");
    assert_eq!(totals[1]["remaining_nano"], "280000000000");

    // One profile missing its weekly row makes the whole weekly aggregate unknown — a
    // partial sum would silently understate fleet capacity — while the complete 5h window
    // still sums exactly.
    let report = vec![
        measured(
            &subject_one,
            registry::GLM_5H_WINDOW_SECS,
            50_000_000_000,
            25_000_000,
        ),
        measured(
            &subject_one,
            registry::GLM_WEEKLY_WINDOW_SECS,
            200_000_000_000,
            10_000_000,
        ),
        measured(
            &subject_two,
            registry::GLM_5H_WINDOW_SECS,
            30_000_000_000,
            50_000_000,
        ),
    ];
    let value = glm_subs_value_with_report(&gateway, &status, 1_800_000_100, Some(&report));
    let totals = value["window_totals"].as_array().unwrap();
    assert_eq!(totals[0]["capacity_nano"], "80000000000");
    assert!(totals[1]["capacity_nano"].is_null());
    assert!(totals[1]["remaining_nano"].is_null());
}

#[test]
fn prometheus_glm_series_are_zero_gauges_without_a_plane_and_never_labelled() {
    let mut body = String::new();
    write_glm_operational_metrics(&mut body, false, None);
    for sample in [
        "claude_api_glm_enabled 0",
        "claude_api_glm_profiles 0",
        "claude_api_glm_live_profiles 0",
        "claude_api_glm_available_profiles 0",
        "claude_api_glm_inflight_requests 0",
        "claude_api_glm_account_dead_profiles 0",
        "claude_api_glm_account_suspect_profiles 0",
        "claude_api_glm_transport_cooling_profiles 0",
        "claude_api_glm_quota_cooling_profiles 0",
        "claude_api_glm_calibration_pending_events 0",
        "claude_api_glm_calibration_dropped_events_total 0",
        "claude_api_glm_calibration_persistence_ok 0",
        "claude_api_glm_missing_terminal_usage_total 0",
        "claude_api_glm_served_model_rejected_total 0",
    ] {
        assert!(body.contains(sample), "missing {sample}");
    }
    // Never observed: the timestamp series is omitted entirely rather than emitted as 0.
    assert!(!body.contains("claude_api_glm_quota_last_observation_timestamp_seconds"));
    for line in body
        .lines()
        .filter(|line| line.starts_with("claude_api_glm") && !line.starts_with('#'))
    {
        assert!(
            !line.contains('{'),
            "glm series must carry no labels at all: {line}"
        );
    }
}

#[test]
fn prometheus_glm_series_report_fleet_aggregates_and_the_freshest_observation() {
    let status = unknown_glm_status();
    let mut body = String::new();
    write_glm_operational_metrics(&mut body, true, Some(&status));
    for sample in [
        "claude_api_glm_enabled 1",
        "claude_api_glm_profiles 1",
        "claude_api_glm_live_profiles 1",
        "claude_api_glm_available_profiles 1",
        "claude_api_glm_inflight_requests 2",
        "claude_api_glm_account_dead_profiles 0",
        "claude_api_glm_account_suspect_profiles 0",
        "claude_api_glm_transport_cooling_profiles 0",
        "claude_api_glm_quota_cooling_profiles 0",
        "claude_api_glm_calibration_pending_events 1",
        "claude_api_glm_calibration_dropped_events_total 0",
        "claude_api_glm_calibration_persistence_ok 1",
        "claude_api_glm_missing_terminal_usage_total 3",
        "claude_api_glm_served_model_rejected_total 2",
        "claude_api_glm_quota_last_observation_timestamp_seconds 1800000000",
    ] {
        assert!(body.contains(sample), "missing {sample}");
    }
    for line in body
        .lines()
        .filter(|line| line.starts_with("claude_api_glm") && !line.starts_with('#'))
    {
        assert!(
            !line.contains('{'),
            "glm series must carry no labels at all: {line}"
        );
    }
}

#[tokio::test]
async fn metrics_endpoint_publishes_label_free_glm_zero_gauges_for_a_disabled_plane() {
    let service = router(admin_auth_test_app(), Arc::new(AtomicBool::new(true)));
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
    let mut request = Request::builder()
        .uri("/metrics")
        .header("x-api-key", "panel-key")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = String::from_utf8(
        to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(body.contains("claude_api_glm_enabled 0"));
    assert!(body.contains("claude_api_glm_profiles 0"));
    assert!(!body.contains("claude_api_glm_quota_last_observation_timestamp_seconds"));
    for line in body
        .lines()
        .filter(|line| line.starts_with("claude_api_glm") && !line.starts_with('#'))
    {
        assert!(
            !line.contains('{'),
            "glm series must carry no labels at all: {line}"
        );
    }
}

#[test]
fn unsupported_openai_subroutes_use_generic_openai_error_shape() {
    let error = unsupported_openai_endpoint_error();
    assert_eq!(error["error"]["type"], "invalid_request_error");
    assert_eq!(
        error["error"]["message"],
        "The requested endpoint is not supported."
    );
    let serialized = error.to_string();
    assert!(!serialized.contains("Codex"));
    assert!(!serialized.contains("app-server"));
    assert!(!serialized.contains("ChatGPT"));
    assert!(!serialized.contains("Anthropic"));
}

#[test]
fn customer_error_event_is_structured_and_redacts_request_data() {
    let uri: Uri = "/v1/responses/secret-response/input_items?api_key=raw-secret"
        .parse()
        .unwrap();
    let event = customer_error_event(
        StatusCode::PAYMENT_REQUIRED,
        "billing_limit",
        "acct_safe",
        "key_safe",
        &Method::POST,
        &uri,
        "request_safe",
        Some(60),
        Some(&registry::AccountRow {
            id: "acct_safe".to_string(),
            handle: None,
            balance_nano: 999,
            spent_nano: 1,
            reserved_nano: 2,
            uncollected_nano: 0,
            mult_bp: 500,
            status: "active".to_string(),
        }),
        &registry::KeyRow {
            key: "raw-key-must-not-appear".to_string(),
            key_id: "key_safe".to_string(),
            account_id: Some("acct_safe".to_string()),
            label: Some("private label must not appear".to_string()),
            spent_nano: 3,
            reserved_nano: 4,
            spend_limit_nano: None,
            expires_ts: None,
            created_ts: 0,
            last_used_ts: None,
            status: "active".to_string(),
        },
    );
    let value: Value = serde_json::from_str(&event).unwrap();
    assert_eq!(
        value,
        json!({
            "event": "customer_http_error",
            "status": 402,
            "reason": "billing_limit",
            "account_id": "acct_safe",
            "key_id": "key_safe",
            "method": "POST",
            "path": "/v1/responses/{id}/input_items",
            "request_id": "request_safe",
            "retry_after_seconds": 60,
            "account_balance_nano": 999,
            "account_reserved_nano": 2,
            "key_spent_nano": 3,
            "key_reserved_nano": 4,
            "key_spend_limit_nano": null,
            "key_expires_ts": null,
            "account_status": "active",
            "key_status": "active",
        })
    );
    assert!(!event.contains("secret-response"));
    assert!(!event.contains("raw-secret"));
    assert!(!event.contains("api_key"));
    assert!(!event.contains("raw-key-must-not-appear"));
    assert!(!event.contains("private label must not appear"));
}

#[test]
fn audit_path_allows_only_fixed_route_templates() {
    assert_eq!(audit_path("/v1/messages"), "/v1/messages");
    assert_eq!(
        audit_path("/v1/images/generations"),
        "/v1/images/generations"
    );
    assert_eq!(audit_path("/v1/images/edits"), "/v1/images/edits");
    assert_eq!(
        audit_path("/v1/models/client-controlled"),
        "/v1/models/{id}"
    );
    assert_eq!(
        audit_path("/v1/client-secret/unsupported"),
        "/v1/{unsupported}"
    );
}

#[test]
fn billing_limit_reason_identifies_the_binding_budget() {
    let account = registry::AccountRow {
        id: "acct_safe".to_string(),
        handle: None,
        balance_nano: 1_000,
        spent_nano: 0,
        reserved_nano: 0,
        uncollected_nano: 0,
        mult_bp: 500,
        status: "active".to_string(),
    };
    let mut key = registry::KeyRow {
        key: "secret".to_string(),
        key_id: "key_safe".to_string(),
        account_id: Some(account.id.clone()),
        label: None,
        spent_nano: 300,
        reserved_nano: 200,
        spend_limit_nano: None,
        expires_ts: None,
        created_ts: 0,
        last_used_ts: None,
        status: "active".to_string(),
    };
    assert_eq!(
        billing_limit_reason(Some(&account), &key),
        "account_balance"
    );
    key.spend_limit_nano = Some(700);
    assert_eq!(
        billing_limit_reason(Some(&account), &key),
        "key_spend_limit"
    );
    key.spend_limit_nano = Some(2_000);
    assert_eq!(
        billing_limit_reason(Some(&account), &key),
        "account_balance"
    );
    key.spend_limit_nano = Some(1_500);
    assert_eq!(
        billing_limit_reason(Some(&account), &key),
        "account_and_key_limit"
    );
}

#[test]
fn positive_balance_402_counter_predicate_requires_both_facts() {
    let mut account = registry::AccountRow {
        id: "acct_safe".to_string(),
        handle: None,
        balance_nano: 1,
        spent_nano: 0,
        reserved_nano: 0,
        uncollected_nano: 0,
        mult_bp: 10_000,
        status: "active".to_string(),
    };
    assert!(is_positive_balance_402(
        StatusCode::PAYMENT_REQUIRED,
        Some(&account),
    ));
    assert!(!is_positive_balance_402(
        StatusCode::BAD_REQUEST,
        Some(&account)
    ));
    account.balance_nano = 0;
    assert!(!is_positive_balance_402(
        StatusCode::PAYMENT_REQUIRED,
        Some(&account),
    ));
    assert!(!is_positive_balance_402(StatusCode::PAYMENT_REQUIRED, None));
}

fn fleet_history_test_app(tag: &str) -> (AppState, std::path::PathBuf) {
    let mut app = admin_auth_test_app();
    // metrics.db открывается рядом с data_db_path — направляем каталог в tempdir, чтобы
    // тест не оставлял файл в рабочем дереве крейта.
    let dir = std::env::temp_dir().join(format!("fh_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    app.data_db_path = Arc::new(dir.join("data.db").to_string_lossy().into_owned());
    (app, dir)
}

#[tokio::test]
async fn fleet_history_enforces_control_key_and_validates_window() {
    let (app, dir) = fleet_history_test_app("gate");
    let service = router(app, Arc::new(AtomicBool::new(true)));
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
    // В ряду денежные агрегаты (balance/spent) → гейт control, read-only panel-ключ не подходит.
    for (credential, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some("panel-key"), StatusCode::UNAUTHORIZED),
        (Some("control-key"), StatusCode::OK),
        (Some("admin-key"), StatusCode::OK),
    ] {
        let mut request = Request::builder()
            .uri("/fleet-history")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        if let Some(key) = credential {
            request
                .headers_mut()
                .insert("x-api-key", key.parse().unwrap());
        }
        let response = service.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), expected, "credential {credential:?}");
        if expected == StatusCode::OK {
            let body = to_bytes(response.into_body(), 65_536).await.unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["window"], "7d", "дефолтное окно — 7d");
            assert_eq!(body["bucket_secs"], 1_800);
            assert_eq!(body["series"], json!([]), "истории ещё нет — пустой ряд");
            assert!(body["now"].as_i64().unwrap() > 0);
        }
    }
    for uri in [
        "/fleet-history?window=3d",
        "/fleet-history?window=",
        "/fleet-history?sub=%0A",
    ] {
        let mut request = Request::builder()
            .uri(uri)
            .header("x-api-key", "control-key")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        let response = service.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{uri}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn fleet_history_returns_bucketed_fleet_and_per_sub_series() {
    let (app, dir) = fleet_history_test_app("series");
    let service = router(app, Arc::new(AtomicBool::new(true)));
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
    // Сеем три минутных снапшота (как poller::metrics_loop) + per-sub ряд одной подписки.
    let now = pool::now();
    let c = crate::metrics_store::open(dir.join("metrics.db").to_str().unwrap()).unwrap();
    for mins_ago in [0i64, 1, 2] {
        let ts = now - mins_ago * 60;
        crate::metrics_store::insert_snapshot(
            &c,
            &serde_json::json!({
                "now": ts, "subs": 3, "calibrated": true,
                "supply": {"avail_usd": {"1h": 10.0, "5h": 20.0, "1d": 30.0, "7d": 40.0},
                           "cap_usd": {"5h": 20.0, "7d": 100.0},
                           "consumed_usd": {"5h": 1.0, "7d": 5.0},
                           "util": {"5h": 0.05, "7d": 0.5}, "health": {"healthy": 2, "cooling": 1}},
                "demand": {"balance_usd": 500.0, "reserved_usd": 1.0, "spent_usd": 9.0,
                           "active_accounts": 4, "potential_realapi_usd": 2500.0},
                "headroom": {"5h": null, "7d": 8.0}, "coverage": {"7d": 62.5},
                "recommend": {"subs_needed": 1, "gap": -2}
            }),
        )
        .unwrap();
        crate::metrics_store::insert_sub_snapshots(
            &c,
            ts,
            &[("alpha@example.com".to_string(), 10.0, 100.0, 0.2, 0.4)],
        )
        .unwrap();
    }
    drop(c);
    // Флот-ряд: все поля контракта на месте, значения из снапшотов.
    let mut request = Request::builder()
        .uri("/fleet-history?window=24h")
        .header("x-api-key", "control-key")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 65_536).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["window"], "24h");
    assert_eq!(body["bucket_secs"], 300);
    let series = body["series"].as_array().unwrap();
    assert!(!series.is_empty(), "сеяные снапшоты должны попасть в ряд");
    let point = &series[0];
    for field in [
        "ts",
        "avail_1h",
        "avail_5h",
        "avail_1d",
        "avail_7d",
        "util5h",
        "util7d",
        "cap5h",
        "cap7d",
        "cons5h",
        "cons7d",
        "healthy",
        "cooling",
        "subs",
        "balance_usd",
        "reserved_usd",
        "spent_usd",
        "potential_realapi",
        "coverage7d",
        "headroom5h",
        "headroom7d",
        "subs_needed",
        "gap",
    ] {
        assert!(point.get(field).is_some(), "нет поля {field}");
    }
    assert_eq!(point["avail_5h"], 20.0);
    assert_eq!(point["balance_usd"], 500.0);
    assert_eq!(point["gap"], -2);
    assert_eq!(point["subs"], 3);
    assert!(
        point["headroom5h"].is_null(),
        "headroom5h=∞ хранится как NULL → null"
    );
    assert_eq!(point["headroom7d"], 8.0);
    // Per-sub ряд по маске «alph…» (URL-encoded «…»), как его шлёт панель.
    let mut request = Request::builder()
        .uri("/fleet-history?window=24h&sub=alph%E2%80%A6")
        .header("x-api-key", "control-key")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 65_536).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["sub"], "alph…");
    let series = body["series"].as_array().unwrap();
    assert!(!series.is_empty());
    assert_eq!(series[0]["cap7d"], 100.0);
    assert_eq!(series[0]["cap5h"], 10.0);
    assert_eq!(series[0]["util7d"], 0.4);
    // Чужая маска → пустой ряд, полные email в ответе не светятся.
    let mut request = Request::builder()
        .uri("/fleet-history?window=24h&sub=zzzz")
        .header("x-api-key", "control-key")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.clone().oneshot(request).await.unwrap();
    let body = to_bytes(response.into_body(), 65_536).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["series"], json!([]));
    assert!(!body.to_string().contains("alpha@example.com"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// AppState с настоящим SQLite-биллингом в tempdir — для /spend-stats и /settlement-health
/// (AsyncBilling::start сам поднимает writer + reader на том же файле, WAL делит чтения).
fn billing_test_app(tag: &str) -> (AppState, std::path::PathBuf) {
    let mut app = admin_auth_test_app();
    let dir = std::env::temp_dir().join(format!("billing_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("data.db");
    let billing = forward::AsyncBilling::start(db.to_string_lossy().into_owned(), 1).unwrap();
    billing.set_admin_changes(app.admin_changes.clone());
    app.billing = Some(Arc::new(billing));
    (app, dir)
}

async fn router_auth_request(service: &Router, credential: Option<&str>) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri("/internal/router/auth/preflight")
        .body(Body::empty())
        .unwrap();
    if let Some(credential) = credential {
        request
            .headers_mut()
            .insert("x-api-key", credential.parse().unwrap());
    }
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424))));
    let response = service.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

async fn router_policy_request(
    service: &Router,
    credential: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri("/internal/router/policy/preflight")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    if let Some(credential) = credential {
        request
            .headers_mut()
            .insert("x-api-key", credential.parse().unwrap());
    }
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424))));
    let response = service.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

async fn router_pricing_request(
    service: &Router,
    credential: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri("/internal/router/catalog/pricing")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    if let Some(credential) = credential {
        request
            .headers_mut()
            .insert("x-api-key", credential.parse().unwrap());
    }
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424))));
    let response = service.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 256 * 1024).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

fn pricing_candidates() -> Value {
    json!({
        "schema_version": 1,
        "candidates": [
            {
                "id": "anthropic/claude-sonnet-4-6",
                "provider_id": "anthropic",
                "model_id": "claude-sonnet-4-6"
            },
            {
                "id": "openai/gpt-5.6-sol",
                "provider_id": "openai",
                "model_id": "gpt-5.6-sol"
            },
            {
                "id": "google/gemini-3.6-flash",
                "provider_id": "google",
                "model_id": "gemini-3.6-flash"
            }
        ]
    })
}

fn policy_candidates() -> Value {
    json!({
        "schema_version": 1,
        "candidates": [
            {
                "id": "anthropic/claude-sonnet-5",
                "provider_id": "anthropic",
                "canonical_model_id": "claude-sonnet-5"
            },
            {
                "id": "openai/gpt-5.6-sol",
                "provider_id": "openai",
                "canonical_model_id": "gpt-5.6-sol"
            },
            {
                "id": "google/gemini-3.6-flash",
                "provider_id": "google",
                "canonical_model_id": "gemini-3.6-flash"
            }
        ]
    })
}

#[tokio::test]
async fn router_auth_preflight_is_bodyless_read_only_and_present_on_every_plane() {
    for provider in [
        forward::ProviderMode::Combined,
        forward::ProviderMode::Anthropic,
        forward::ProviderMode::OpenAi,
        forward::ProviderMode::Gemini,
        forward::ProviderMode::Kimi,
    ] {
        let service = router(provider_test_app(provider), Arc::new(AtomicBool::new(true)));
        let (status, body) = router_auth_request(&service, Some("admin-key")).await;
        assert_eq!(status, StatusCode::OK, "provider={provider:?}");
        assert_eq!(body, json!({"schema_version": 1, "authenticated": true}));
    }

    let service = router(
        provider_test_app(forward::ProviderMode::Anthropic),
        Arc::new(AtomicBool::new(true)),
    );
    let (status, body) = router_auth_request(&service, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");

    let (app, dir) = billing_test_app("router_auth_preflight");
    let conn = registry::open(dir.join("data.db").to_str().unwrap()).unwrap();
    registry::account_create(&conn, "router-auth-account", None, 10_000).unwrap();
    registry::key_issue(
        &conn,
        "router-auth-key",
        "router-auth-account",
        Some("router auth"),
    )
    .unwrap();
    drop(conn);

    let service = router(app, Arc::new(AtomicBool::new(true)));
    let (status, body) = router_auth_request(&service, Some("router-auth-key")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"schema_version": 1, "authenticated": true}));
    assert!(!body.to_string().contains("router-auth-account"));

    let conn = registry::open(dir.join("data.db").to_str().unwrap()).unwrap();
    registry::key_set_status(&conn, "router-auth-key", "inactive").unwrap();
    drop(conn);
    let (status, body) = router_auth_request(&service, Some("router-auth-key")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");

    let (status, body) = router_auth_request(&service, Some("unknown-key")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");
    drop(service);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn router_policy_preflight_is_present_on_every_plane_and_admin_is_unrestricted() {
    for provider in [
        forward::ProviderMode::Combined,
        forward::ProviderMode::Anthropic,
        forward::ProviderMode::OpenAi,
        forward::ProviderMode::Gemini,
        forward::ProviderMode::Kimi,
    ] {
        let service = router(provider_test_app(provider), Arc::new(AtomicBool::new(true)));
        let (status, body) =
            router_policy_request(&service, Some("admin-key"), policy_candidates()).await;
        assert_eq!(status, StatusCode::OK, "provider={provider:?}");
        assert_eq!(body["schema_version"], 1);
        assert_eq!(body["mode"], "unrestricted");
        assert_eq!(
            body["allowed"],
            json!([
                "anthropic/claude-sonnet-5",
                "openai/gpt-5.6-sol",
                "google/gemini-3.6-flash"
            ])
        );
    }

    let service = router(
        provider_test_app(forward::ProviderMode::Anthropic),
        Arc::new(AtomicBool::new(true)),
    );
    let (status, body) = router_policy_request(&service, None, policy_candidates()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");

    let mut malformed = policy_candidates();
    malformed["authority_override"] = json!(true);
    let (status, body) = router_policy_request(&service, Some("admin-key"), malformed).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn router_catalog_pricing_is_key_scoped_integer_only_and_present_on_every_plane() {
    for provider in [
        forward::ProviderMode::Combined,
        forward::ProviderMode::Anthropic,
        forward::ProviderMode::OpenAi,
        forward::ProviderMode::Gemini,
        forward::ProviderMode::Kimi,
    ] {
        let service = router(provider_test_app(provider), Arc::new(AtomicBool::new(true)));
        let (status, body) =
            router_pricing_request(&service, Some("admin-key"), pricing_candidates()).await;
        assert_eq!(status, StatusCode::OK, "provider={provider:?}");
        assert_eq!(body["schema_version"], 1);
        assert_eq!(body["unit"], "nano_usd_per_million_tokens");
        assert_eq!(body["mode"], "admin");
        assert_eq!(body["entries"].as_array().unwrap().len(), 3);
        assert_eq!(body["entries"][0]["standard"]["input"], "3000000000");
        assert_eq!(body["entries"][1]["priority"]["output"], "60000000000");
        assert!(body["entries"][0]["standard"]["input"].is_string());
    }

    let (app, dir) = billing_test_app("router_catalog_pricing");
    let conn = registry::open(dir.join("data.db").to_str().unwrap()).unwrap();
    registry::account_create(&conn, "discount-account", None, 5_000).unwrap();
    registry::key_issue(
        &conn,
        "discount-key",
        "discount-account",
        Some("router pricing"),
    )
    .unwrap();
    drop(conn);

    let service = router(app, Arc::new(AtomicBool::new(true)));
    let (status, body) =
        router_pricing_request(&service, Some("discount-key"), pricing_candidates()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["mode"], "legacy");
    assert_eq!(body["entries"][0]["standard"]["input"], "1500000000");
    assert_eq!(body["entries"][1]["standard"]["output"], "15000000000");
    assert_eq!(body["entries"][1]["priority"]["output"], "30000000000");
    assert_eq!(body["entries"][2]["standard"]["input"], "375000000");
    assert!(!body.to_string().contains("discount-account"));

    let (status, body) =
        router_pricing_request(&service, Some("unknown-key"), pricing_candidates()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");
    drop(service);
    let _ = std::fs::remove_dir_all(&dir);
}

/// /spend-stats кэширует periods в процессном static, а cargo test гоняет тесты параллельно:
/// без сериализации соседний тест получил бы periods чужого tempdir-биллинга. Гард держится
/// до конца теста и на захвате сбрасывает кэш.
fn spend_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = LOCK.lock().unwrap();
    if let Some(cell) = SPEND_CACHE.get() {
        *cell.lock().unwrap() = None;
    }
    guard
}

#[tokio::test]
async fn settlement_health_enforces_control_key_and_reports_pipeline() {
    let (app, dir) = billing_test_app("settlement");
    // Сеем outbox напрямую: свежий failed с длинным last_error + старый pending (backlog).
    let conn = registry::open(dir.join("data.db").to_str().unwrap()).unwrap();
    let ts = pool::now();
    conn.execute(
        "INSERT INTO billing_settlement_outbox(request_id,actual_nano,state,attempts, \
         next_attempt_ts,last_error,created_ts,updated_ts) \
         VALUES('r-failed',1500000000,'failed',5,0,?1,?2,?3)",
        rusqlite::params!["x".repeat(500), ts - 7200, ts - 30],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO billing_settlement_outbox(request_id,actual_nano,state,attempts, \
         next_attempt_ts,last_error,created_ts,updated_ts) \
         VALUES('r-stuck',1000,'pending',3,0,'transient pg error',?1,?2)",
        rusqlite::params![ts - 3600, ts - 60],
    )
    .unwrap();
    drop(conn);
    let service = router(app, Arc::new(AtomicBool::new(true)));
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
    // Денежная диагностика → гейт control, read-only panel-ключ не подходит.
    for (credential, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some("panel-key"), StatusCode::UNAUTHORIZED),
        (Some("control-key"), StatusCode::OK),
        (Some("admin-key"), StatusCode::OK),
    ] {
        let mut request = Request::builder()
            .uri("/settlement-health")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        if let Some(key) = credential {
            request
                .headers_mut()
                .insert("x-api-key", key.parse().unwrap());
        }
        let response = service.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), expected, "credential {credential:?}");
    }
    let mut request = Request::builder()
        .uri("/settlement-health")
        .header("x-api-key", "control-key")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 65_536).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert!(body["now"].as_i64().unwrap() > 0);
    assert_eq!(body["backlog_threshold_secs"], 300);
    let outbox = &body["outbox"];
    assert_eq!(outbox["failed"], 1);
    assert_eq!(outbox["failed_24h"], 1);
    assert_eq!(outbox["pending"], 1);
    assert_eq!(outbox["pending_with_error"], 1);
    assert_eq!(outbox["backlog"], 1, "старый pending старше порога");
    assert!(outbox["oldest_unsettled_age_secs"].as_i64().unwrap() >= 3000);
    let failed = outbox["recent_failed"].as_array().unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0]["request_id"], "r-failed");
    assert_eq!(failed[0]["actual_usd"], 1.5);
    assert_eq!(failed[0]["attempts"], 5);
    assert_eq!(
        failed[0]["last_error"].as_str().unwrap().chars().count(),
        200,
        "last_error урезан до 200 символов"
    );
    let consumer = &body["pricing_consumer"];
    assert_eq!(consumer["consumer"], "pricing");
    for field in [
        "ledger_max_id",
        "checkpoints",
        "checkpoint_min",
        "unacked",
        "oldest_unacked_ts",
    ] {
        assert!(consumer.get(field).is_some(), "нет поля {field}");
    }
    assert_eq!(consumer["checkpoints"], 0, "консьюмер ещё не ack-ал");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn spend_stats_includes_served_model_breakdown() {
    let (app, dir) = billing_test_app("spend");
    let _lock = spend_test_lock();
    let conn = registry::open(dir.join("data.db").to_str().unwrap()).unwrap();
    registry::account_create(&conn, "acct", None, 2000).unwrap();
    let usage = registry::UsageEventInput {
        model: "claude-opus-5".into(),
        real_nano: 100_000_000,
        charge_basis_nano: 100_000_000,
        ..Default::default()
    };
    registry::usage_event_add(&conn, "acct", None, &usage, 50_000_000, Some("r1")).unwrap();
    registry::usage_event_add(&conn, "acct", None, &usage, 25_000_000, Some("r2")).unwrap();
    drop(conn);
    let service = router(app, Arc::new(AtomicBool::new(true)));
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
    let mut request = Request::builder()
        .uri("/spend-stats")
        .header("x-api-key", "control-key")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 65_536).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    let d1 = &body["periods"]["d1"];
    let models = d1["models"].as_array().unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0]["model"], "claude-opus-5");
    assert_eq!(models[0]["provider"], "anthropic");
    assert_eq!(models[0]["requests"], 2);
    assert_eq!(models[0]["charge_usd"], 0.08); // (50M+25M) nano → $0.075 → 0.08
    assert_eq!(models[0]["real_usd"], 0.2);
    // accounts/providers не потерялись рядом с новой разбивкой.
    assert_eq!(d1["accounts"].as_array().unwrap().len(), 1);
    assert_eq!(d1["providers"].as_array().unwrap().len(), 1);
    // Без ?from&to произвольного диапазона в ответе нет.
    assert!(body.get("custom").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn spend_stats_validates_custom_range() {
    let (app, dir) = billing_test_app("spend_range_bad");
    let _lock = spend_test_lock();
    let service = router(app, Arc::new(AtomicBool::new(true)));
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
    let now = pool::now();
    let mut bad = vec![
        "/spend-stats?from=abc&to=123".to_string(),
        "/spend-stats?from=100&to=xyz".to_string(),
        "/spend-stats?from=100".to_string(),
        "/spend-stats?to=100".to_string(),
        "/spend-stats?from=200&to=100".to_string(),
        "/spend-stats?from=-5&to=100".to_string(),
        // шире 92 дней даже после зажатия to до now
        "/spend-stats?from=0&to=99999999999".to_string(),
        // диапазон целиком в будущем
        format!("/spend-stats?from={}&to={}", now + 3_600, now + 7_200),
    ];
    // Гейт идёт первым: без ключа даже мусорные параметры отвечают 401, а не 400.
    let mut request = Request::builder()
        .uri(bad[0].as_str())
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    for uri in bad.drain(..) {
        let mut request = Request::builder()
            .uri(uri.as_str())
            .header("x-api-key", "control-key")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        let response = service.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "uri {uri}");
        let body = to_bytes(response.into_body(), 65_536).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert!(body["error"].as_str().unwrap().len() > 10, "uri {uri}");
    }
    // Валидный диапазон на пустых данных: custom присутствует с нулевыми суммами,
    // to из будущего зажимается (внутренний код кладёт now+1 — проверяем только границы).
    let uri = format!("/spend-stats?from={}&to={}", now - 3_600, now + 3_600);
    let mut request = Request::builder()
        .uri(uri.as_str())
        .header("x-api-key", "control-key")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 65_536).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    let custom = &body["custom"];
    assert_eq!(custom["from"], now - 3_600);
    assert_eq!(custom["requests"], 0);
    assert_eq!(custom["accounts"], json!([]));
    assert_eq!(custom["providers"], json!([]));
    assert_eq!(custom["models"], json!([]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn spend_stats_custom_range_aggregates_window() {
    let (app, dir) = billing_test_app("spend_custom");
    let _lock = spend_test_lock();
    let conn = registry::open(dir.join("data.db").to_str().unwrap()).unwrap();
    registry::account_create(&conn, "acct", None, 2000).unwrap();
    let usage = registry::UsageEventInput {
        model: "claude-opus-5".into(),
        real_nano: 100_000_000,
        charge_basis_nano: 100_000_000,
        ..Default::default()
    };
    registry::usage_event_add(&conn, "acct", None, &usage, 50_000_000, Some("r1")).unwrap();
    registry::usage_event_add(&conn, "acct", None, &usage, 25_000_000, Some("r2")).unwrap();
    // Старое событие вне диапазона, но внутри стандартного окна d30.
    registry::usage_event_add(&conn, "acct", None, &usage, 70_000_000, Some("r3")).unwrap();
    conn.execute(
        "UPDATE usage_events SET ts=?1 WHERE ref='r3'",
        rusqlite::params![pool::now() - 10 * 86_400],
    )
    .unwrap();
    drop(conn);
    let service = router(app, Arc::new(AtomicBool::new(true)));
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
    let now = pool::now();
    let uri = format!("/spend-stats?from={}&to={}", now - 3_600, now + 3_600);
    let mut request = Request::builder()
        .uri(uri.as_str())
        .header("x-api-key", "control-key")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 65_536).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    let custom = &body["custom"];
    assert_eq!(custom["from"], now - 3_600);
    assert!(custom["to"].as_i64().unwrap() <= now + 1);
    // В диапазон попали только r1+r2: 75M nano charge → $0.08, 200M real → $0.2.
    assert_eq!(custom["requests"], 2);
    assert_eq!(custom["charge_usd"], 0.08);
    assert_eq!(custom["real_usd"], 0.2);
    let accounts = custom["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0]["account"], "acct");
    assert_eq!(accounts[0]["requests"], 2);
    assert!(accounts[0]["last_ts"].as_i64().unwrap() >= now - 3_600);
    let providers = custom["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0]["provider"], "anthropic");
    let models = custom["models"].as_array().unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0]["model"], "claude-opus-5");
    // Стандартное окно d30 рядом видит и старое событие.
    assert_eq!(body["periods"]["d30"]["requests"], 3);
    // Кэш не загрязнён custom: повторный запрос без параметров отдаёт чистые periods.
    let mut request = Request::builder()
        .uri("/spend-stats")
        .header("x-api-key", "control-key")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 65_536).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert!(body.get("custom").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// An idle healthy subscription that spent a whole 5h window without traffic used exactly nothing.
/// Publishing `null` there discarded a measured zero and made the panel say "обновляем" on a 0%
/// window — the single most misleading state, because it looked like missing evidence.
#[test]
fn claude_rolled_over_window_publishes_exact_zero_without_pricing_money() {
    let now = 2_000_000;
    let mut cap = capacity("idle@example.test", 999.0, true, true);
    cap.quota5h = Some(pool::QuotaSnapshot {
        used_fraction_units: 0,
        measurement_resolution_fraction_units: 100_000,
        observed_at: now - 6 * 3_600,
        resets_at: Some(now - 3_600),
    });
    let report = (
        vec![claude_calibration(
            "idle@example.test",
            "5h",
            0,
            10_000_000,
            1_000_000_000,
        )],
        Vec::new(),
        Vec::new(),
    );

    let value = capacity_value(
        std::slice::from_ref(&cap),
        Some(&report),
        claude_delivery(0),
        now,
    );
    let five = &value["per_sub"][0]["windows"][0];
    assert_eq!(five["used_fraction_units"], 0);
    assert_eq!(five["quota_state"], "window_rolled_over");
    assert_eq!(value["per_sub"][0]["util5h"], 0.0);
    // The window is empty, but its capacity has not been re-measured: money stays null and the
    // fleet total must not treat a rolled-over window as priced supply.
    assert_eq!(five["snapshot_fresh"], false);
    assert!(five["remaining_nano"].is_null());
    assert!(five["remaining_low_nano"].is_null());
    assert!(five["remaining_high_nano"].is_null());
    assert!(five["last_known_remaining_nano"].is_null());
    assert!(value["window_totals"][0]["remaining_nano"].is_null());
    assert!(value["available_nano"]["next_5h"].is_null());
    assert_eq!(five["missing_reason"], "stale_current_quota_snapshot");

    // A cooling window is exhausted, not refilled: its passed reset must not fabricate a zero.
    let mut cooling = cap.clone();
    cooling.cooling = true;
    cooling.routable = false;
    let cooling_value = capacity_value(&[cooling], Some(&report), claude_delivery(0), now);
    assert!(cooling_value["per_sub"][0]["windows"][0]["used_fraction_units"].is_null());
}

/// Provider quota/reset and saleable dollars have independent authorities. A pending or degraded
/// money FIFO used to blank the exact quota fraction too, so an operator lost sight of the real
/// utilization wall precisely while the money path was broken.
#[test]
fn claude_pending_delivery_keeps_quota_visible_while_money_stays_closed() {
    let now = 2_000;
    let mut cap = capacity("wall@example.test", 999.0, true, true);
    cap.quota5h = Some(pool::QuotaSnapshot {
        used_fraction_units: 97_000_000,
        measurement_resolution_fraction_units: 100_000,
        observed_at: now - 10,
        resets_at: Some(now + 1_800),
    });
    let report = (
        vec![claude_calibration(
            "wall@example.test",
            "5h",
            97_000_000,
            10_000_000,
            1_000_000_000,
        )],
        Vec::new(),
        Vec::new(),
    );

    for delivery in [claude_delivery(1), claude_degraded_delivery()] {
        let value = capacity_value(std::slice::from_ref(&cap), Some(&report), delivery, now);
        let five = &value["per_sub"][0]["windows"][0];
        assert_eq!(five["used_fraction_units"], 97_000_000);
        assert_eq!(five["resets_at"], now + 1_800);
        assert_eq!(five["snapshot_fresh"], true);
        assert_eq!(value["per_sub"][0]["reset5h_in"], 1_800);
        // Not one dollar may be published while exact turn evidence is undelivered.
        assert!(five["remaining_nano"].is_null());
        assert!(five["last_known_remaining_nano"].is_null());
        assert!(value["window_totals"][0]["remaining_nano"].is_null());
        assert!(value["available_nano"]["next_5h"].is_null());
        assert!(value["per_sub"][0]["rem5h_nano"].is_null());
    }
}

/// A frozen quota snapshot keeps reporting its last value, so every Anthropic window gauge can look
/// healthy while the refresh path is dead. Only this observation timestamp separates the two, which
/// is what `AnthropicQuotaSnapshotStale` alerts on.
#[tokio::test]
async fn anthropic_quota_snapshot_freshness_is_published_for_alerting() {
    let mut app = admin_auth_test_app();
    app.pool = Arc::new(pool::Pool::new(
        vec![registry::Sub {
            email: "metrics@example.test".to_owned(),
            token: "token".to_owned(),
            proxy: String::new(),
            fleet: String::new(),
            plan: "max20".to_owned(),
        }],
        pool::Reserve::new(0.1, 0.03, 0.02),
        0.0,
        0.0,
    ));

    let read_metrics = |app: AppState| async move {
        let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
        let mut request = Request::builder()
            .uri("/metrics")
            .header("x-api-key", "panel-key")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        let response = router(app, Arc::new(AtomicBool::new(true)))
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        String::from_utf8(
            to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap()
    };

    // Never observed: the timestamp is absent rather than zero, so the alert cannot fire on a
    // subscription that simply has not been probed yet.
    let body = read_metrics(app.clone()).await;
    assert!(body.contains("claude_api_anthropic_auth_suspect_subscriptions 0"));
    assert!(body.contains("claude_api_anthropic_auth_dead_subscriptions 0"));
    assert!(body.contains("claude_api_anthropic_quota_snapshot_subscriptions 0"));
    assert!(!body.contains("claude_api_anthropic_quota_last_observation_timestamp_seconds"));

    app.pool.import_health(vec![registry::SubHealth {
        email: "metrics@example.test".to_owned(),
        auth_state: "suspect".to_owned(),
        auth_fail_streak: 1,
        first_auth_fail_ts: 1_799_999_000,
        last_auth_fail_ts: 1_799_999_000,
        last_auth_http: 401,
        auth_token_fp: pool::token_fp("token"),
        ..registry::SubHealth::default()
    }]);
    let body = read_metrics(app.clone()).await;
    assert!(body.contains("claude_api_anthropic_auth_suspect_subscriptions 1"));
    assert!(body.contains("claude_api_anthropic_auth_dead_subscriptions 0"));

    app.pool.import_health(vec![registry::SubHealth {
        email: "metrics@example.test".to_owned(),
        auth_state: "dead".to_owned(),
        auth_fail_streak: 2,
        first_auth_fail_ts: 1_799_999_000,
        last_auth_fail_ts: 1_799_999_300,
        last_auth_http: 401,
        dead_since_ts: 1_799_999_300,
        dead_reason: "authentication_error".to_owned(),
        auth_token_fp: pool::token_fp("token"),
    }]);
    let body = read_metrics(app.clone()).await;
    assert!(body.contains("claude_api_anthropic_auth_suspect_subscriptions 0"));
    assert!(body.contains("claude_api_anthropic_auth_dead_subscriptions 1"));

    app.pool.import_health(vec![registry::SubHealth {
        email: "metrics@example.test".to_owned(),
        auth_state: "healthy".to_owned(),
        auth_token_fp: pool::token_fp("token"),
        ..registry::SubHealth::default()
    }]);

    app.pool.set_quota_snapshots(
        "metrics@example.test",
        Some(pool::QuotaSnapshot {
            used_fraction_units: 25_000_000,
            measurement_resolution_fraction_units: 100_000,
            observed_at: 1_800_000_000,
            resets_at: Some(1_800_001_800),
        }),
        None,
    );
    let body = read_metrics(app).await;
    assert!(body.contains("claude_api_anthropic_quota_snapshot_subscriptions 1"));
    assert!(
        body.contains("claude_api_anthropic_quota_last_observation_timestamp_seconds 1800000000")
    );
}

/// The deploy stops a slot when this gauge reaches zero, so it must not fall to zero while the
/// customer is still being served. Historically the only in-flight numbers were provider-side: they
/// free their lease when the upstream finishes, leaving the body — and the settlement written when
/// it ends — unprotected. That tail is exactly where reservations were abandoned in `delivering`
/// and later charged the full preflight hold.
#[tokio::test]
async fn the_active_request_gauge_spans_the_response_body_not_just_the_handler() {
    let app = provider_test_app(forward::ProviderMode::Gemini);
    let metrics = app.metrics.clone();
    let service = router(app, Arc::new(AtomicBool::new(true)));
    let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));

    assert_eq!(forward::Metrics::get(&metrics.active_requests), 0);

    let mut request = Request::builder()
        .method("GET")
        .uri("/balance")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(peer);
    let response = service.clone().oneshot(request).await.unwrap();

    // The handler has returned, but the body is still owned by the caller: the slot still owes work.
    assert_eq!(forward::Metrics::get(&metrics.active_requests), 1);
    let _ = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(forward::Metrics::get(&metrics.active_requests), 0);

    // A readiness poll must never make a draining slot look busy, or the deploy would never stop it.
    let mut probe = Request::builder()
        .method("GET")
        .uri("/ready")
        .body(Body::empty())
        .unwrap();
    probe.extensions_mut().insert(peer);
    let probe = service.oneshot(probe).await.unwrap();
    let _ = to_bytes(probe.into_body(), usize::MAX).await.unwrap();
    assert_eq!(forward::Metrics::get(&metrics.active_requests), 0);
}
