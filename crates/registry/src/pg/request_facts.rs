use crate::request_facts::{
    BillingOutcome, RequestFactAdmission, RequestFactAxis, RequestFactCursor,
    RequestFactDimensionCount, RequestFactLogicalRows, RequestFactPage, RequestFactReadRow,
    RequestFactReadWindow, RequestFactSummary, RequestFactSummaryTotals,
    RequestFactTerminalEvidence, TerminalRequestFact, MAX_REQUEST_FACT_BATCH,
    MAX_REQUEST_FACT_READ_LIMIT, MAX_REQUEST_FACT_SUMMARY_GROUPS,
    REQUEST_FACT_ADMISSION_SCHEMA_VERSION,
};
use anyhow::{bail, Result};
use postgres::{Row, Transaction};

const ADMISSION_COLUMNS: &str = "logical_request_id,billing_request_id,execution_group_id,attempt,account_id,key_id,client_kind,client_source,client_version,provider_plane,route_class,request_class,requested_model,executable_model,stream_flag,tools_declared_count,tool_classes,tool_choice_mode,parallel_tools_requested,tool_results_in_input,structured_output_flag,reasoning_flag,service_tier,input_modalities,output_modalities,admitted_at,schema_version";

const READ_COLUMNS: &str = "fact_id,logical_request_id,attempt,client_kind,client_source,client_version,provider_plane,route_class,request_class,requested_model,executable_model,stream_flag,tools_declared_count,tool_classes,tool_choice_mode,parallel_tools_requested,tool_results_in_input,structured_output_flag,reasoning_flag,service_tier,input_modalities,output_modalities,admitted_at,delivery_started_at,first_public_byte_at,terminal_at,http_status_code,provider_terminal_class,delivery_state,billing_outcome,downstream_disconnect,internal_attempt_count,tool_calls_in_output,schema_version";

fn safe_duration(start: Option<i64>, end: Option<i64>) -> Option<i64> {
    match (start, end) {
        (Some(start), Some(end)) if end >= start => Some(end - start),
        _ => None,
    }
}

fn read_row(row: Row, include_logical: bool) -> RequestFactReadRow {
    let admitted_at: i64 = row.get(22);
    let delivery_started_at: Option<i64> = row.get(23);
    let first_public_byte_at: Option<i64> = row.get(24);
    let terminal_at: Option<i64> = row.get(25);
    RequestFactReadRow {
        fact_id: row.get(0),
        logical_request_id: include_logical.then(|| row.get(1)),
        attempt: row.get(2),
        client_kind: row.get(3),
        client_source: row.get(4),
        client_version: row.get(5),
        provider_plane: row.get(6),
        route_class: row.get(7),
        request_class: row.get(8),
        requested_model: row.get(9),
        executable_model: row.get(10),
        stream: row.get(11),
        tools_declared_count: row.get(12),
        tool_classes: row.get(13),
        tool_choice_mode: row.get(14),
        parallel_tools_requested: row.get(15),
        tool_results_in_input: row.get(16),
        structured_output: row.get(17),
        reasoning: row.get(18),
        service_tier: row.get(19),
        input_modalities: row.get(20),
        output_modalities: row.get(21),
        admitted_at,
        delivery_started_at,
        first_public_byte_at,
        terminal_at,
        http_status_code: row.get(26),
        provider_terminal_class: row.get(27),
        delivery_state: row.get(28),
        billing_outcome: row.get(29),
        downstream_disconnect: row.get(30),
        internal_attempt_count: row.get(31),
        tool_calls_in_output: row.get(32),
        admission_to_delivery_seconds: safe_duration(Some(admitted_at), delivery_started_at),
        admission_to_first_public_byte_seconds: safe_duration(
            Some(admitted_at),
            first_public_byte_at,
        ),
        delivery_to_first_public_byte_seconds: safe_duration(
            delivery_started_at,
            first_public_byte_at,
        ),
        admission_to_terminal_seconds: safe_duration(Some(admitted_at), terminal_at),
        schema_version: row.get(33),
    }
}

fn count(row: &Row, index: usize) -> Result<u64> {
    u64::try_from(row.get::<_, i64>(index))
        .map_err(|_| anyhow::anyhow!("request-fact aggregate is negative"))
}

fn axis(rows: Vec<Row>, dimensions: usize) -> Result<RequestFactAxis> {
    let truncated = rows.len() > MAX_REQUEST_FACT_SUMMARY_GROUPS as usize;
    let mut groups = Vec::with_capacity(rows.len().min(MAX_REQUEST_FACT_SUMMARY_GROUPS as usize));
    for row in rows
        .into_iter()
        .take(MAX_REQUEST_FACT_SUMMARY_GROUPS as usize)
    {
        let mut values = Vec::with_capacity(dimensions);
        for index in 0..dimensions {
            values.push(row.get(index));
        }
        groups.push(RequestFactDimensionCount {
            values,
            count: count(&row, dimensions)?,
        });
    }
    Ok(RequestFactAxis { groups, truncated })
}

fn account_clause(account_id: Option<&str>, parameter: usize) -> String {
    if account_id.is_some() {
        format!(" AND account_id=${parameter}")
    } else {
        String::new()
    }
}

pub(super) fn summary(
    tx: &mut Transaction<'_>,
    window: RequestFactReadWindow,
    account_id: Option<&str>,
) -> Result<RequestFactSummary> {
    let account = account_clause(account_id, 3);
    let mut params: Vec<&(dyn postgres::types::ToSql + Sync)> = vec![&window.from, &window.to];
    if let Some(ref account_id) = account_id {
        params.push(account_id);
    }
    let totals_sql = format!(
        r#"SELECT COUNT(*)::bigint,
                  COUNT(*) FILTER (WHERE terminal_at IS NOT NULL)::bigint,
                  COUNT(*) FILTER (WHERE terminal_at IS NULL)::bigint,
                  COUNT(*) FILTER (
                    WHERE terminal_at IS NOT NULL
                      AND (schema_version<>1 OR provider_terminal_class IS NULL
                           OR delivery_state IS NULL OR billing_outcome IS NULL)
                  )::bigint
             FROM request_facts
            WHERE admitted_at >= $1 AND admitted_at < $2{account}"#
    );
    let totals = tx.query_one(&totals_sql, &params)?;
    let grouped = |tx: &mut Transaction<'_>, select: &str, group: &str, dimensions: usize| {
        let sql = format!(
            "SELECT {select},COUNT(*)::bigint FROM request_facts WHERE admitted_at >= $1 AND admitted_at < $2{account} GROUP BY {group} ORDER BY COUNT(*) DESC,{group} LIMIT {}",
            MAX_REQUEST_FACT_SUMMARY_GROUPS + 1
        );
        axis(tx.query(&sql, &params)?, dimensions)
    };
    Ok(RequestFactSummary {
        totals: RequestFactSummaryTotals {
            persisted: count(&totals, 0)?,
            terminal: count(&totals, 1)?,
            nonterminal: count(&totals, 2)?,
            required_evidence_unknown: count(&totals, 3)?,
        },
        clients: grouped(
            tx,
            "client_kind,client_source",
            "client_kind,client_source",
            2,
        )?,
        routes: grouped(
            tx,
            "provider_plane,route_class,request_class",
            "provider_plane,route_class,request_class",
            3,
        )?,
        requested_models: grouped(tx, "requested_model", "requested_model", 1)?,
        executable_models: grouped(tx, "executable_model", "executable_model", 1)?,
        terminal_classes: grouped(tx, "provider_terminal_class", "provider_terminal_class", 1)?,
        delivery_states: grouped(tx, "delivery_state", "delivery_state", 1)?,
        billing_outcomes: grouped(tx, "billing_outcome", "billing_outcome", 1)?,
    })
}

pub(super) fn page(
    tx: &mut Transaction<'_>,
    window: RequestFactReadWindow,
    account_id: Option<&str>,
    cursor: Option<RequestFactCursor>,
    limit: usize,
) -> Result<RequestFactPage> {
    if !(1..=MAX_REQUEST_FACT_READ_LIMIT).contains(&limit) {
        bail!("request-fact limit is outside 1..200");
    }
    let account = account_clause(account_id, 3);
    let cursor_clause = if account_id.is_some() { 4 } else { 3 };
    let mut sql = format!(
        "SELECT {READ_COLUMNS} FROM request_facts WHERE admitted_at >= $1 AND admitted_at < $2{account}"
    );
    let mut params: Vec<&(dyn postgres::types::ToSql + Sync)> = vec![&window.from, &window.to];
    if let Some(ref account_id) = account_id {
        params.push(account_id);
    }
    let cursor_ts;
    let cursor_id;
    if let Some(cursor) = cursor {
        cursor_ts = cursor.admitted_at;
        cursor_id = cursor.fact_id;
        sql.push_str(&format!(
            " AND (admitted_at < ${cursor_clause} OR (admitted_at = ${cursor_clause} AND fact_id < ${}))",
            cursor_clause + 1
        ));
        params.push(&cursor_ts);
        params.push(&cursor_id);
    }
    let page_limit = i64::try_from(limit + 1)?;
    sql.push_str(&format!(
        " ORDER BY admitted_at DESC,fact_id DESC LIMIT ${}",
        params.len() + 1
    ));
    params.push(&page_limit);
    let mut rows = tx
        .query(&sql, &params)?
        .into_iter()
        .map(|row| read_row(row, false))
        .collect::<Vec<_>>();
    let has_more = rows.len() > limit;
    if has_more {
        rows.truncate(limit);
    }
    let next = has_more && !rows.is_empty();
    let next = next.then(|| {
        let last = rows.last().expect("nonempty page");
        RequestFactCursor {
            admitted_at: last.admitted_at,
            fact_id: last.fact_id,
        }
    });
    Ok(RequestFactPage { rows, next })
}

pub(super) fn logical(
    tx: &mut Transaction<'_>,
    logical_request_id: &str,
) -> Result<RequestFactLogicalRows> {
    let limit = i64::try_from(MAX_REQUEST_FACT_READ_LIMIT + 1)?;
    let sql = format!(
        "SELECT {READ_COLUMNS} FROM request_facts WHERE logical_request_id=$1 ORDER BY attempt ASC,fact_id ASC LIMIT $2"
    );
    let mut rows = tx
        .query(&sql, &[&logical_request_id, &limit])?
        .into_iter()
        .map(|row| read_row(row, true))
        .collect::<Vec<_>>();
    let truncated = rows.len() > MAX_REQUEST_FACT_READ_LIMIT;
    if truncated {
        rows.truncate(MAX_REQUEST_FACT_READ_LIMIT);
    }
    Ok(RequestFactLogicalRows { rows, truncated })
}

pub(super) fn validate_reservation_fact(
    fact: &RequestFactAdmission,
    request_id: &str,
    account_id: &str,
    key_id: &str,
    execution_group_id: Option<&str>,
    attempt: i32,
) -> Result<()> {
    fact.validate()?;
    if fact.billing_request_id != request_id
        || fact.account_id != account_id
        || fact.key_id != key_id
        || fact.execution_group_id.as_deref() != execution_group_id
        || fact.attempt != attempt
    {
        bail!("request-fact admission conflicts with reservation identity");
    }
    Ok(())
}

fn admission_matches(row: &Row, fact: &RequestFactAdmission) -> bool {
    row.get::<_, String>(0) == fact.logical_request_id
        && row.get::<_, Option<String>>(1).as_deref() == Some(fact.billing_request_id.as_str())
        && row.get::<_, Option<String>>(2).as_deref() == fact.execution_group_id.as_deref()
        && row.get::<_, i32>(3) == fact.attempt
        && row.get::<_, String>(4) == fact.account_id
        && row.get::<_, String>(5) == fact.key_id
        && row.get::<_, String>(6) == fact.client_kind.as_str()
        && row.get::<_, String>(7) == fact.client_source.as_str()
        && row.get::<_, Option<String>>(8).as_deref() == fact.client_version.as_deref()
        && row.get::<_, String>(9) == fact.provider_plane
        && row.get::<_, String>(10) == fact.route_class
        && row.get::<_, String>(11) == fact.request_class
        && row.get::<_, Option<String>>(12).as_deref() == fact.requested_model.as_deref()
        && row.get::<_, Option<String>>(13).as_deref() == fact.executable_model.as_deref()
        && row.get::<_, bool>(14) == fact.stream_flag
        && row.get::<_, Option<i32>>(15) == fact.tools_declared_count
        && row.get::<_, Option<i32>>(16) == fact.tool_classes
        && row.get::<_, Option<String>>(17).as_deref()
            == fact.tool_choice_mode.map(|value| value.as_str())
        && row.get::<_, Option<bool>>(18) == fact.parallel_tools_requested
        && row.get::<_, Option<bool>>(19) == fact.tool_results_in_input
        && row.get::<_, Option<bool>>(20) == fact.structured_output_flag
        && row.get::<_, Option<bool>>(21) == fact.reasoning_flag
        && row.get::<_, Option<String>>(22).as_deref() == fact.service_tier.as_deref()
        && row.get::<_, Option<i32>>(23) == fact.input_modalities
        && row.get::<_, Option<i32>>(24) == fact.output_modalities
        && row.get::<_, i64>(25) == fact.admitted_at
        && row.get::<_, i32>(26) == REQUEST_FACT_ADMISSION_SCHEMA_VERSION
}

pub(super) fn insert_or_validate_admission(
    tx: &mut Transaction<'_>,
    fact: &RequestFactAdmission,
) -> Result<()> {
    let inserted = tx.query_opt(
        r#"INSERT INTO request_facts(
            logical_request_id,billing_request_id,execution_group_id,attempt,account_id,key_id,
            client_kind,client_source,client_version,provider_plane,route_class,request_class,
            requested_model,executable_model,stream_flag,tools_declared_count,tool_classes,
            tool_choice_mode,parallel_tools_requested,tool_results_in_input,structured_output_flag,
            reasoning_flag,service_tier,input_modalities,output_modalities,admitted_at,schema_version)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,
                $22,$23,$24,$25,$26,$27)
         ON CONFLICT(billing_request_id) DO NOTHING RETURNING fact_id"#,
        &[
            &fact.logical_request_id,
            &fact.billing_request_id,
            &fact.execution_group_id,
            &fact.attempt,
            &fact.account_id,
            &fact.key_id,
            &fact.client_kind.as_str(),
            &fact.client_source.as_str(),
            &fact.client_version,
            &fact.provider_plane,
            &fact.route_class,
            &fact.request_class,
            &fact.requested_model,
            &fact.executable_model,
            &fact.stream_flag,
            &fact.tools_declared_count,
            &fact.tool_classes,
            &fact.tool_choice_mode.map(|value| value.as_str()),
            &fact.parallel_tools_requested,
            &fact.tool_results_in_input,
            &fact.structured_output_flag,
            &fact.reasoning_flag,
            &fact.service_tier,
            &fact.input_modalities,
            &fact.output_modalities,
            &fact.admitted_at,
            &REQUEST_FACT_ADMISSION_SCHEMA_VERSION,
        ],
    )?;
    if inserted.is_none() {
        let sql =
            format!("SELECT {ADMISSION_COLUMNS} FROM request_facts WHERE billing_request_id=$1");
        let row = tx.query_one(&sql, &[&fact.billing_request_id])?;
        if !admission_matches(&row, fact) {
            bail!("billing request ID conflicts with different request-fact admission");
        }
    }
    Ok(())
}

pub(super) fn validate_existing_admission(
    tx: &mut Transaction<'_>,
    fact: &RequestFactAdmission,
) -> Result<()> {
    let sql = format!("SELECT {ADMISSION_COLUMNS} FROM request_facts WHERE billing_request_id=$1");
    let row = tx
        .query_opt(&sql, &[&fact.billing_request_id])?
        .ok_or_else(|| anyhow::anyhow!("reservation replay lacks its request-fact admission"))?;
    if !admission_matches(&row, fact) {
        bail!("billing request ID conflicts with different request-fact admission");
    }
    Ok(())
}

pub(super) fn mark_delivery_started(
    tx: &mut Transaction<'_>,
    request_id: &str,
    delivery_started_at: i64,
) -> Result<()> {
    let fact = tx.query_opt(
        "SELECT admitted_at,delivery_started_at FROM request_facts \
         WHERE billing_request_id=$1 FOR UPDATE",
        &[&request_id],
    )?;
    let Some(fact) = fact else {
        return Ok(());
    };
    let admitted_at: i64 = fact.get(0);
    if delivery_started_at < admitted_at {
        bail!("request-fact delivery time precedes admission");
    }
    if fact.get::<_, Option<i64>>(1).is_none() {
        tx.execute(
            "UPDATE request_facts SET delivery_started_at=$2 WHERE billing_request_id=$1",
            &[&request_id, &delivery_started_at],
        )?;
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DurableTerminalEnvelope {
    pub schema_version: i32,
    pub evidence: RequestFactTerminalEvidence,
}

impl DurableTerminalEnvelope {
    pub(super) fn from_outbox_row(
        row: &Row,
        offset: usize,
        admitted_at: i64,
        delivery_started_at: Option<i64>,
    ) -> Result<Option<Self>> {
        let schema_version: Option<i32> = row.get(offset);
        let terminal_at: Option<i64> = row.get(offset + 1);
        let http_status_code: Option<i32> = row.get(offset + 2);
        let provider_terminal_class: Option<String> = row.get(offset + 3);
        let delivery_state: Option<String> = row.get(offset + 4);
        let downstream_disconnect: Option<bool> = row.get(offset + 5);
        let upstream_request_id: Option<String> = row.get(offset + 6);
        let first_public_byte_at: Option<i64> = row.get(offset + 7);
        let internal_attempt_count: Option<i32> = row.get(offset + 8);
        let failure_class: Option<String> = row.get(offset + 9);
        let tool_calls_in_output: Option<bool> = row.get(offset + 10);
        let all_null = schema_version.is_none()
            && terminal_at.is_none()
            && http_status_code.is_none()
            && provider_terminal_class.is_none()
            && delivery_state.is_none()
            && downstream_disconnect.is_none()
            && upstream_request_id.is_none()
            && first_public_byte_at.is_none()
            && internal_attempt_count.is_none()
            && failure_class.is_none()
            && tool_calls_in_output.is_none();
        if all_null {
            return Ok(None);
        }
        let schema_version = schema_version
            .ok_or_else(|| anyhow::anyhow!("durable request-fact envelope lacks schema version"))?;
        if schema_version != crate::request_facts::REQUEST_FACT_TERMINAL_SCHEMA_VERSION {
            bail!("unsupported durable request-fact terminal schema version");
        }
        let evidence = RequestFactTerminalEvidence {
            terminal_at: terminal_at.ok_or_else(|| {
                anyhow::anyhow!("durable request-fact envelope lacks terminal time")
            })?,
            http_status_code,
            provider_terminal_class: crate::request_facts::ProviderTerminalClass::parse(
                provider_terminal_class.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("durable request-fact envelope lacks provider class")
                })?,
            )?,
            delivery_state: crate::request_facts::DeliveryState::parse(
                delivery_state.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("durable request-fact envelope lacks delivery state")
                })?,
            )?,
            downstream_disconnect,
            upstream_request_id,
            first_public_byte_at,
            internal_attempt_count,
            failure_class,
            tool_calls_in_output,
        };
        evidence.validate_with_delivery(admitted_at, delivery_started_at)?;
        Ok(Some(Self {
            schema_version,
            evidence,
        }))
    }
}

pub(super) fn finalize_terminal(
    tx: &mut Transaction<'_>,
    request_id: &str,
    envelope: &DurableTerminalEnvelope,
    outcome: BillingOutcome,
) -> Result<Option<crate::request_facts::RequestFactLifecycleObservation>> {
    let existing = tx.query_opt(
        r#"SELECT terminal_at,http_status_code,provider_terminal_class,delivery_state,
                downstream_disconnect,upstream_request_id,first_public_byte_at,
                internal_attempt_count,failure_class,tool_calls_in_output,billing_outcome,
                admitted_at,delivery_started_at,provider_plane,route_class,request_class,stream_flag
           FROM request_facts WHERE billing_request_id=$1 FOR UPDATE"#,
        &[&request_id],
    )?;
    let Some(row) = existing else {
        return Ok(None);
    };
    envelope
        .evidence
        .validate_with_delivery(row.get(11), row.get(12))?;
    let terminal_at: Option<i64> = row.get(0);
    if terminal_at.is_none() {
        tx.execute(
            r#"UPDATE request_facts
                SET terminal_at=$2,http_status_code=$3,provider_terminal_class=$4,
                    delivery_state=$5,downstream_disconnect=$6,upstream_request_id=$7,
                    first_public_byte_at=$8,internal_attempt_count=$9,failure_class=$10,
                    tool_calls_in_output=$11,billing_outcome=$12
              WHERE billing_request_id=$1 AND terminal_at IS NULL"#,
            &[
                &request_id,
                &envelope.evidence.terminal_at,
                &envelope.evidence.http_status_code,
                &envelope.evidence.provider_terminal_class.as_str(),
                &envelope.evidence.delivery_state.as_str(),
                &envelope.evidence.downstream_disconnect,
                &envelope.evidence.upstream_request_id,
                &envelope.evidence.first_public_byte_at,
                &envelope.evidence.internal_attempt_count,
                &envelope.evidence.failure_class,
                &envelope.evidence.tool_calls_in_output,
                &outcome.as_str(),
            ],
        )?;
        return Ok(Some(
            crate::request_facts::RequestFactLifecycleObservation {
                provider_plane: row.get(13),
                route_class: row.get(14),
                request_class: row.get(15),
                stream: row.get(16),
                admitted_at: row.get(11),
                delivery_started_at: row.get(12),
                terminal: envelope.evidence.clone(),
            },
        ));
    }
    let exact = terminal_at == Some(envelope.evidence.terminal_at)
        && row.get::<_, Option<i32>>(1) == envelope.evidence.http_status_code
        && row.get::<_, Option<String>>(2).as_deref()
            == Some(envelope.evidence.provider_terminal_class.as_str())
        && row.get::<_, Option<String>>(3).as_deref()
            == Some(envelope.evidence.delivery_state.as_str())
        && row.get::<_, Option<bool>>(4) == envelope.evidence.downstream_disconnect
        && row.get::<_, Option<String>>(5).as_deref()
            == envelope.evidence.upstream_request_id.as_deref()
        && row.get::<_, Option<i64>>(6) == envelope.evidence.first_public_byte_at
        && row.get::<_, Option<i32>>(7) == envelope.evidence.internal_attempt_count
        && row.get::<_, Option<String>>(8).as_deref() == envelope.evidence.failure_class.as_deref()
        && row.get::<_, Option<bool>>(9) == envelope.evidence.tool_calls_in_output
        && row.get::<_, Option<String>>(10).as_deref() == Some(outcome.as_str());
    if !exact {
        bail!("terminal request fact conflicts with durable outbox outcome");
    }
    Ok(None)
}

pub(super) fn insert_terminal_batch(
    tx: &mut Transaction<'_>,
    facts: &[TerminalRequestFact],
) -> Result<(
    usize,
    Vec<crate::request_facts::RequestFactLifecycleObservation>,
)> {
    if facts.len() > MAX_REQUEST_FACT_BATCH {
        bail!("request-fact terminal batch exceeds hard cap");
    }
    for fact in facts {
        fact.validate()?;
    }
    let mut inserted = 0;
    let mut observations = Vec::new();
    for fact in facts {
        let billing_outcome = BillingOutcome::NotApplicable.as_str();
        let row_inserted = tx.execute(
            r#"INSERT INTO request_facts(
                logical_request_id,billing_request_id,execution_group_id,attempt,account_id,key_id,
                client_kind,client_source,client_version,provider_plane,route_class,request_class,
                requested_model,executable_model,stream_flag,tools_declared_count,tool_classes,
                tool_choice_mode,parallel_tools_requested,tool_results_in_input,structured_output_flag,
                reasoning_flag,service_tier,input_modalities,output_modalities,admitted_at,terminal_at,
                http_status_code,provider_terminal_class,delivery_state,billing_outcome,
                downstream_disconnect,upstream_request_id,first_public_byte_at,internal_attempt_count,
                failure_class,tool_calls_in_output,schema_version)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,
                    $22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32,$33,$34,$35,$36,$37,$38)
             ON CONFLICT DO NOTHING"#,
            &[
                &fact.logical_request_id,
                &fact.billing_request_id,
                &fact.execution_group_id,
                &fact.attempt,
                &fact.account_id,
                &fact.key_id,
                &fact.client_kind.as_str(),
                &fact.client_source.as_str(),
                &fact.client_version,
                &fact.provider_plane,
                &fact.route_class,
                &fact.request_class,
                &fact.requested_model,
                &fact.executable_model,
                &fact.stream_flag,
                &fact.tools_declared_count,
                &fact.tool_classes,
                &fact.tool_choice_mode.map(|value| value.as_str()),
                &fact.parallel_tools_requested,
                &fact.tool_results_in_input,
                &fact.structured_output_flag,
                &fact.reasoning_flag,
                &fact.service_tier,
                &fact.input_modalities,
                &fact.output_modalities,
                &fact.admitted_at,
                &fact.terminal.terminal_at,
                &fact.terminal.http_status_code,
                &fact.terminal.provider_terminal_class.as_str(),
                &fact.terminal.delivery_state.as_str(),
                &billing_outcome,
                &fact.terminal.downstream_disconnect,
                &fact.terminal.upstream_request_id,
                &fact.terminal.first_public_byte_at,
                &fact.terminal.internal_attempt_count,
                &fact.terminal.failure_class,
                &fact.terminal.tool_calls_in_output,
                &REQUEST_FACT_ADMISSION_SCHEMA_VERSION,
            ],
        )? as usize;
        inserted += row_inserted;
        if row_inserted == 1 {
            observations.push(crate::request_facts::RequestFactLifecycleObservation {
                provider_plane: fact.provider_plane.clone(),
                route_class: fact.route_class.clone(),
                request_class: fact.request_class.clone(),
                stream: fact.stream_flag,
                admitted_at: fact.admitted_at,
                delivery_started_at: None,
                terminal: fact.terminal.clone(),
            });
        }
    }
    Ok((inserted, observations))
}

pub(super) fn prune_first(tx: &mut Transaction<'_>, older_than_ts: i64) -> Result<usize> {
    Ok(tx.execute(
        "DELETE FROM request_facts WHERE fact_id IN ( \
           SELECT fact_id FROM request_facts WHERE admitted_at < $1 \
           ORDER BY admitted_at,fact_id LIMIT 5000 \
         )",
        &[&older_than_ts],
    )? as usize)
}
