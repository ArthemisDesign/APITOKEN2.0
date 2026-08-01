//! Exact provider-turn and Claude window calibration persistence.
//!
//! Estimation remains a `forward` concern. This module only validates immutable integer evidence,
//! advances the cumulative provider-subject spend ledger atomically with a winning event insert,
//! and CAS-persists already-derived Claude window state together with its raw observation.

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OptionalExtension};

use crate::{PROVIDER_ANTHROPIC, PROVIDER_GOOGLE};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderTurnCalibrationEvent {
    pub provider: String,
    pub request_id: String,
    pub subject_id: String,
    pub model_id: String,
    pub service_tier: String,
    pub inference_geo: String,
    pub tariff_schedule_id: String,
    pub priced_ts: i64,
    pub completed_at: i64,
    pub input_tokens: i64,
    pub audio_input_tokens: i64,
    pub cache_read_tokens: i64,
    pub cached_audio_input_tokens: i64,
    pub cache_write_5m_tokens: i64,
    pub cache_write_1h_tokens: i64,
    pub output_tokens: i64,
    pub thinking_output_tokens: i64,
    pub image_output_tokens: i64,
    pub tool_prompt_tokens: i64,
    pub search_queries: i64,
    pub grounded_search_prompts: i64,
    pub api_input_nanousd: i64,
    pub api_audio_input_nanousd: i64,
    pub api_cache_read_nanousd: i64,
    pub api_cached_audio_input_nanousd: i64,
    pub api_cache_write_5m_nanousd: i64,
    pub api_cache_write_1h_nanousd: i64,
    pub api_output_nanousd: i64,
    pub api_image_output_nanousd: i64,
    pub api_search_nanousd: i64,
    pub api_total_nanousd: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderCalibrationSubjectSpend {
    pub spent_nano: i64,
    pub tracking_started_ts: Option<i64>,
    pub updated_ts: Option<i64>,
    /// True only when this call inserted the immutable event and advanced cumulative spend.
    pub inserted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderTurnCalibrationAggregate {
    pub provider: String,
    pub subject_id: String,
    pub model_id: String,
    pub service_tier: String,
    pub inference_geo: String,
    pub tariff_schedule_id: String,
    pub turns: i64,
    pub first_completed_at: i64,
    pub last_completed_at: i64,
    pub input_tokens: i64,
    pub audio_input_tokens: i64,
    pub cache_read_tokens: i64,
    pub cached_audio_input_tokens: i64,
    pub cache_write_5m_tokens: i64,
    pub cache_write_1h_tokens: i64,
    pub output_tokens: i64,
    pub thinking_output_tokens: i64,
    pub image_output_tokens: i64,
    pub tool_prompt_tokens: i64,
    pub search_queries: i64,
    pub grounded_search_prompts: i64,
    pub api_input_nanousd: i64,
    pub api_audio_input_nanousd: i64,
    pub api_cache_read_nanousd: i64,
    pub api_cached_audio_input_nanousd: i64,
    pub api_cache_write_5m_nanousd: i64,
    pub api_cache_write_1h_nanousd: i64,
    pub api_output_nanousd: i64,
    pub api_image_output_nanousd: i64,
    pub api_search_nanousd: i64,
    pub api_total_nanousd: i64,
}

#[derive(Debug)]
pub struct ProviderTurnCalibrationReplayConflict;

impl std::fmt::Display for ProviderTurnCalibrationReplayConflict {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("provider calibration request id replay conflict")
    }
}

impl std::error::Error for ProviderTurnCalibrationReplayConflict {}

pub fn is_provider_turn_calibration_replay_conflict(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.is::<ProviderTurnCalibrationReplayConflict>())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnthropicCalibrationRow {
    pub subject_id: String,
    pub plan: String,
    pub window_kind: String,
    pub window_duration_mins: i64,
    pub resets_at: i64,
    pub anchor_used_fraction_units: i64,
    pub anchor_resolution_fraction_units: i64,
    pub anchor_spend_nano: i64,
    pub used_fraction_units: i64,
    pub measurement_resolution_fraction_units: i64,
    pub observed_at: i64,
    pub observed_fraction_units: i64,
    pub observed_spend_nano: i64,
    pub samples: i64,
    pub unattributed_fraction_units: i64,
    pub current_capacity_nano: Option<i64>,
    pub current_low_nano: Option<i64>,
    pub current_high_nano: Option<i64>,
    pub current_confidence_bp: i64,
    pub last_measured_at: Option<i64>,
    pub estimator_version: i64,
    pub version: i64,
    pub updated_ts: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnthropicWindowObservation {
    pub subject_id: String,
    pub plan: String,
    pub window_kind: String,
    pub window_duration_mins: i64,
    pub resets_at: i64,
    pub observed_at: i64,
    pub used_fraction_units: i64,
    pub measurement_resolution_fraction_units: i64,
    pub gateway_spend_nano: i64,
    pub observation_source: String,
    pub source_request_id: Option<String>,
}

pub(crate) fn validate_provider_turn_calibration_event(
    event: &ProviderTurnCalibrationEvent,
) -> Result<()> {
    let valid_provider = matches!(
        event.provider.as_str(),
        PROVIDER_ANTHROPIC | PROVIDER_GOOGLE
    );
    let token_legs = [
        event.input_tokens,
        event.audio_input_tokens,
        event.cache_read_tokens,
        event.cached_audio_input_tokens,
        event.cache_write_5m_tokens,
        event.cache_write_1h_tokens,
        event.output_tokens,
        event.thinking_output_tokens,
        event.image_output_tokens,
        event.tool_prompt_tokens,
        event.search_queries,
        event.grounded_search_prompts,
    ];
    let cost_legs = [
        event.api_input_nanousd,
        event.api_audio_input_nanousd,
        event.api_cache_read_nanousd,
        event.api_cached_audio_input_nanousd,
        event.api_cache_write_5m_nanousd,
        event.api_cache_write_1h_nanousd,
        event.api_output_nanousd,
        event.api_image_output_nanousd,
        event.api_search_nanousd,
    ];
    let total = cost_legs
        .iter()
        .try_fold(0i128, |sum, value| sum.checked_add(i128::from(*value)))
        .context("provider calibration cost sum overflow")?;
    if !valid_provider
        || event.request_id.is_empty()
        || event.subject_id.is_empty()
        || event.model_id.is_empty()
        || !matches!(event.service_tier.as_str(), "standard" | "fast")
        || !matches!(event.inference_geo.as_str(), "global" | "us")
        || event.tariff_schedule_id.is_empty()
        || event.priced_ts <= 0
        || event.completed_at <= 0
        || token_legs.iter().any(|value| *value < 0)
        || token_legs.iter().all(|value| *value == 0)
        || cost_legs.iter().any(|value| *value < 0)
        || event.api_total_nanousd <= 0
        || total != i128::from(event.api_total_nanousd)
        || event.cached_audio_input_tokens > event.cache_read_tokens
        || event.thinking_output_tokens > event.output_tokens
        || event.tool_prompt_tokens > event.input_tokens
    {
        bail!("invalid provider turn calibration event");
    }
    Ok(())
}

fn valid_anthropic_window(kind: &str, duration_mins: i64) -> bool {
    matches!((kind, duration_mins), ("5h", 300) | ("7d", 10_080))
}

pub fn validate_anthropic_calibration_row(row: &AnthropicCalibrationRow) -> Result<()> {
    let empty_evidence = row.samples == 0
        && row.observed_fraction_units == 0
        && row.observed_spend_nano == 0
        && row.current_capacity_nano.is_none()
        && row.current_low_nano.is_none()
        && row.current_high_nano.is_none()
        && row.current_confidence_bp == 0
        && row.last_measured_at.is_none();
    let measured_evidence = row.samples > 0
        && row.observed_fraction_units > 0
        && row.observed_spend_nano > 0
        && row.current_capacity_nano.is_some()
        && row.current_low_nano.is_some()
        && row.last_measured_at.is_some();
    if row.subject_id.is_empty()
        || row.plan.is_empty()
        || !valid_anthropic_window(&row.window_kind, row.window_duration_mins)
        || row.resets_at <= 0
        || !(0..=100_000_000).contains(&row.anchor_used_fraction_units)
        || !(1..=100_000_000).contains(&row.anchor_resolution_fraction_units)
        || row.anchor_spend_nano < 0
        || !(0..=100_000_000).contains(&row.used_fraction_units)
        || !(1..=100_000_000).contains(&row.measurement_resolution_fraction_units)
        || row.observed_at <= 0
        || row.observed_fraction_units < 0
        || row.observed_spend_nano < 0
        || row.samples < 0
        || row.unattributed_fraction_units < 0
        || row.current_capacity_nano.is_some_and(|value| value < 0)
        || row.current_low_nano.is_some_and(|value| value < 0)
        || row.current_high_nano.is_some_and(|value| value < 0)
        || row
            .current_low_nano
            .zip(row.current_capacity_nano)
            .is_some_and(|(low, capacity)| low > capacity)
        || row
            .current_high_nano
            .zip(row.current_capacity_nano)
            .is_some_and(|(high, capacity)| high < capacity)
        || row
            .current_low_nano
            .zip(row.current_high_nano)
            .is_some_and(|(low, high)| low > high)
        || !(0..=10_000).contains(&row.current_confidence_bp)
        || row.last_measured_at.is_some_and(|value| value <= 0)
        || row.estimator_version <= 0
        || row.version < 0
        || row.updated_ts <= 0
        || !(empty_evidence || measured_evidence)
        || row.current_high_nano.is_none() && row.current_confidence_bp != 0
    {
        bail!("invalid Anthropic calibration row");
    }
    Ok(())
}

pub(crate) fn validate_anthropic_window_observation(
    observation: &AnthropicWindowObservation,
) -> Result<()> {
    let valid_source = match observation.observation_source.as_str() {
        "response" => observation
            .source_request_id
            .as_ref()
            .is_some_and(|request_id| !request_id.is_empty()),
        "poll" => observation.source_request_id.is_none(),
        _ => false,
    };
    if observation.subject_id.is_empty()
        || observation.plan.is_empty()
        || !valid_anthropic_window(&observation.window_kind, observation.window_duration_mins)
        || observation.resets_at <= 0
        || observation.observed_at <= 0
        || !(0..=100_000_000).contains(&observation.used_fraction_units)
        || !(1..=100_000_000).contains(&observation.measurement_resolution_fraction_units)
        || observation.gateway_spend_nano < 0
        || !valid_source
    {
        bail!("invalid Anthropic calibration observation");
    }
    Ok(())
}

pub(crate) fn validate_anthropic_calibration_pair(
    state: &AnthropicCalibrationRow,
    observation: &AnthropicWindowObservation,
) -> Result<()> {
    validate_anthropic_calibration_row(state)?;
    validate_anthropic_window_observation(observation)?;
    if state.subject_id != observation.subject_id
        || state.plan != observation.plan
        || state.window_kind != observation.window_kind
        || state.window_duration_mins != observation.window_duration_mins
        || state.resets_at != observation.resets_at
        || state.used_fraction_units != observation.used_fraction_units
        || state.measurement_resolution_fraction_units
            != observation.measurement_resolution_fraction_units
        || state.observed_at != observation.observed_at
        || state.updated_ts != observation.observed_at
    {
        bail!("Anthropic calibration state/observation mismatch");
    }
    Ok(())
}

pub(crate) const PROVIDER_TURN_EVENT_COLUMNS: &str = "provider,request_id,subject_id,model_id,\
    service_tier,inference_geo,tariff_schedule_id,priced_ts,completed_at,input_tokens,\
    audio_input_tokens,cache_read_tokens,cached_audio_input_tokens,cache_write_5m_tokens,\
    cache_write_1h_tokens,output_tokens,thinking_output_tokens,image_output_tokens,\
    tool_prompt_tokens,search_queries,grounded_search_prompts,api_input_nanousd,\
    api_audio_input_nanousd,api_cache_read_nanousd,api_cached_audio_input_nanousd,\
    api_cache_write_5m_nanousd,api_cache_write_1h_nanousd,api_output_nanousd,\
    api_image_output_nanousd,api_search_nanousd,api_total_nanousd";

fn sqlite_provider_turn_event(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ProviderTurnCalibrationEvent> {
    Ok(ProviderTurnCalibrationEvent {
        provider: row.get(0)?,
        request_id: row.get(1)?,
        subject_id: row.get(2)?,
        model_id: row.get(3)?,
        service_tier: row.get(4)?,
        inference_geo: row.get(5)?,
        tariff_schedule_id: row.get(6)?,
        priced_ts: row.get(7)?,
        completed_at: row.get(8)?,
        input_tokens: row.get(9)?,
        audio_input_tokens: row.get(10)?,
        cache_read_tokens: row.get(11)?,
        cached_audio_input_tokens: row.get(12)?,
        cache_write_5m_tokens: row.get(13)?,
        cache_write_1h_tokens: row.get(14)?,
        output_tokens: row.get(15)?,
        thinking_output_tokens: row.get(16)?,
        image_output_tokens: row.get(17)?,
        tool_prompt_tokens: row.get(18)?,
        search_queries: row.get(19)?,
        grounded_search_prompts: row.get(20)?,
        api_input_nanousd: row.get(21)?,
        api_audio_input_nanousd: row.get(22)?,
        api_cache_read_nanousd: row.get(23)?,
        api_cached_audio_input_nanousd: row.get(24)?,
        api_cache_write_5m_nanousd: row.get(25)?,
        api_cache_write_1h_nanousd: row.get(26)?,
        api_output_nanousd: row.get(27)?,
        api_image_output_nanousd: row.get(28)?,
        api_search_nanousd: row.get(29)?,
        api_total_nanousd: row.get(30)?,
    })
}

pub fn record_provider_turn_calibration_event(
    conn: &Connection,
    event: &ProviderTurnCalibrationEvent,
) -> Result<ProviderCalibrationSubjectSpend> {
    validate_provider_turn_calibration_event(event)?;
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .context("begin SQLite provider turn calibration event")?;
    let inserted = tx.execute(
        "INSERT INTO provider_turn_calibration_events(\
           provider,request_id,subject_id,model_id,service_tier,inference_geo,tariff_schedule_id,\
           priced_ts,completed_at,input_tokens,audio_input_tokens,cache_read_tokens,\
           cached_audio_input_tokens,cache_write_5m_tokens,cache_write_1h_tokens,output_tokens,\
           thinking_output_tokens,image_output_tokens,tool_prompt_tokens,search_queries,\
           grounded_search_prompts,api_input_nanousd,api_audio_input_nanousd,\
           api_cache_read_nanousd,api_cached_audio_input_nanousd,api_cache_write_5m_nanousd,\
           api_cache_write_1h_nanousd,api_output_nanousd,api_image_output_nanousd,\
           api_search_nanousd,api_total_nanousd) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,\
                ?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31) \
         ON CONFLICT(provider,request_id) DO NOTHING",
        rusqlite::params![
            event.provider,
            event.request_id,
            event.subject_id,
            event.model_id,
            event.service_tier,
            event.inference_geo,
            event.tariff_schedule_id,
            event.priced_ts,
            event.completed_at,
            event.input_tokens,
            event.audio_input_tokens,
            event.cache_read_tokens,
            event.cached_audio_input_tokens,
            event.cache_write_5m_tokens,
            event.cache_write_1h_tokens,
            event.output_tokens,
            event.thinking_output_tokens,
            event.image_output_tokens,
            event.tool_prompt_tokens,
            event.search_queries,
            event.grounded_search_prompts,
            event.api_input_nanousd,
            event.api_audio_input_nanousd,
            event.api_cache_read_nanousd,
            event.api_cached_audio_input_nanousd,
            event.api_cache_write_5m_nanousd,
            event.api_cache_write_1h_nanousd,
            event.api_output_nanousd,
            event.api_image_output_nanousd,
            event.api_search_nanousd,
            event.api_total_nanousd,
        ],
    )? == 1;
    if inserted {
        tx.execute(
            "INSERT INTO provider_calibration_subject_spend(\
               provider,subject_id,spent_nano,tracking_started_ts,updated_ts) \
             VALUES(?1,?2,?3,?4,?4) ON CONFLICT(provider,subject_id) DO UPDATE SET \
               spent_nano=provider_calibration_subject_spend.spent_nano+excluded.spent_nano, \
               tracking_started_ts=MIN(provider_calibration_subject_spend.tracking_started_ts,\
                   excluded.tracking_started_ts), \
               updated_ts=MAX(provider_calibration_subject_spend.updated_ts,excluded.updated_ts)",
            rusqlite::params![
                event.provider,
                event.subject_id,
                event.api_total_nanousd,
                event.completed_at,
            ],
        )?;
    } else {
        let existing = tx.query_row(
            &format!(
                "SELECT {PROVIDER_TURN_EVENT_COLUMNS} \
                 FROM provider_turn_calibration_events WHERE provider=?1 AND request_id=?2"
            ),
            rusqlite::params![event.provider, event.request_id],
            sqlite_provider_turn_event,
        )?;
        if existing != *event {
            return Err(ProviderTurnCalibrationReplayConflict.into());
        }
    }
    let mut spend = tx.query_row(
        "SELECT spent_nano,tracking_started_ts,updated_ts \
         FROM provider_calibration_subject_spend WHERE provider=?1 AND subject_id=?2",
        rusqlite::params![event.provider, event.subject_id],
        |row| {
            Ok(ProviderCalibrationSubjectSpend {
                spent_nano: row.get(0)?,
                tracking_started_ts: Some(row.get(1)?),
                updated_ts: Some(row.get(2)?),
                inserted: false,
            })
        },
    )?;
    spend.inserted = inserted;
    tx.commit()?;
    Ok(spend)
}

pub fn provider_calibration_subject_spend(
    conn: &Connection,
    provider: &str,
    subject_id: &str,
) -> Result<ProviderCalibrationSubjectSpend> {
    if !matches!(provider, PROVIDER_ANTHROPIC | PROVIDER_GOOGLE) || subject_id.is_empty() {
        bail!("invalid provider calibration subject");
    }
    Ok(conn
        .query_row(
            "SELECT spent_nano,tracking_started_ts,updated_ts \
             FROM provider_calibration_subject_spend WHERE provider=?1 AND subject_id=?2",
            rusqlite::params![provider, subject_id],
            |row| {
                Ok(ProviderCalibrationSubjectSpend {
                    spent_nano: row.get(0)?,
                    tracking_started_ts: Some(row.get(1)?),
                    updated_ts: Some(row.get(2)?),
                    inserted: false,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

pub fn provider_turn_calibration_report(
    conn: &Connection,
    provider: &str,
) -> Result<Vec<ProviderTurnCalibrationAggregate>> {
    if !matches!(provider, PROVIDER_ANTHROPIC | PROVIDER_GOOGLE) {
        bail!("invalid provider calibration report provider");
    }
    let mut statement = conn.prepare(
        "SELECT provider,subject_id,model_id,service_tier,inference_geo,tariff_schedule_id,\
           COUNT(*),MIN(completed_at),MAX(completed_at),SUM(input_tokens),\
           SUM(audio_input_tokens),SUM(cache_read_tokens),SUM(cached_audio_input_tokens),\
           SUM(cache_write_5m_tokens),SUM(cache_write_1h_tokens),SUM(output_tokens),\
           SUM(thinking_output_tokens),SUM(image_output_tokens),SUM(tool_prompt_tokens),\
           SUM(search_queries),SUM(grounded_search_prompts),SUM(api_input_nanousd),\
           SUM(api_audio_input_nanousd),SUM(api_cache_read_nanousd),\
           SUM(api_cached_audio_input_nanousd),SUM(api_cache_write_5m_nanousd),\
           SUM(api_cache_write_1h_nanousd),SUM(api_output_nanousd),\
           SUM(api_image_output_nanousd),SUM(api_search_nanousd),SUM(api_total_nanousd) \
         FROM provider_turn_calibration_events WHERE provider=?1 \
         GROUP BY provider,subject_id,model_id,service_tier,inference_geo,tariff_schedule_id \
         ORDER BY subject_id,model_id,service_tier,inference_geo,tariff_schedule_id",
    )?;
    let rows = statement
        .query_map(rusqlite::params![provider], |row| {
            Ok(ProviderTurnCalibrationAggregate {
                provider: row.get(0)?,
                subject_id: row.get(1)?,
                model_id: row.get(2)?,
                service_tier: row.get(3)?,
                inference_geo: row.get(4)?,
                tariff_schedule_id: row.get(5)?,
                turns: row.get(6)?,
                first_completed_at: row.get(7)?,
                last_completed_at: row.get(8)?,
                input_tokens: row.get(9)?,
                audio_input_tokens: row.get(10)?,
                cache_read_tokens: row.get(11)?,
                cached_audio_input_tokens: row.get(12)?,
                cache_write_5m_tokens: row.get(13)?,
                cache_write_1h_tokens: row.get(14)?,
                output_tokens: row.get(15)?,
                thinking_output_tokens: row.get(16)?,
                image_output_tokens: row.get(17)?,
                tool_prompt_tokens: row.get(18)?,
                search_queries: row.get(19)?,
                grounded_search_prompts: row.get(20)?,
                api_input_nanousd: row.get(21)?,
                api_audio_input_nanousd: row.get(22)?,
                api_cache_read_nanousd: row.get(23)?,
                api_cached_audio_input_nanousd: row.get(24)?,
                api_cache_write_5m_nanousd: row.get(25)?,
                api_cache_write_1h_nanousd: row.get(26)?,
                api_output_nanousd: row.get(27)?,
                api_image_output_nanousd: row.get(28)?,
                api_search_nanousd: row.get(29)?,
                api_total_nanousd: row.get(30)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub(crate) const ANTHROPIC_CALIBRATION_COLUMNS: &str = "subject_id,plan,window_kind,\
    window_duration_mins,resets_at,anchor_used_fraction_units,\
    anchor_resolution_fraction_units,anchor_spend_nano,used_fraction_units,\
    measurement_resolution_fraction_units,observed_at,observed_fraction_units,\
    observed_spend_nano,samples,unattributed_fraction_units,current_capacity_nano,\
    current_low_nano,current_high_nano,current_confidence_bp,last_measured_at,\
    estimator_version,version,updated_ts";

fn sqlite_anthropic_calibration_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AnthropicCalibrationRow> {
    Ok(AnthropicCalibrationRow {
        subject_id: row.get(0)?,
        plan: row.get(1)?,
        window_kind: row.get(2)?,
        window_duration_mins: row.get(3)?,
        resets_at: row.get(4)?,
        anchor_used_fraction_units: row.get(5)?,
        anchor_resolution_fraction_units: row.get(6)?,
        anchor_spend_nano: row.get(7)?,
        used_fraction_units: row.get(8)?,
        measurement_resolution_fraction_units: row.get(9)?,
        observed_at: row.get(10)?,
        observed_fraction_units: row.get(11)?,
        observed_spend_nano: row.get(12)?,
        samples: row.get(13)?,
        unattributed_fraction_units: row.get(14)?,
        current_capacity_nano: row.get(15)?,
        current_low_nano: row.get(16)?,
        current_high_nano: row.get(17)?,
        current_confidence_bp: row.get(18)?,
        last_measured_at: row.get(19)?,
        estimator_version: row.get(20)?,
        version: row.get(21)?,
        updated_ts: row.get(22)?,
    })
}

pub fn load_anthropic_calibration(
    conn: &Connection,
    subject_id: &str,
    plan: &str,
    window_kind: &str,
) -> Result<Option<AnthropicCalibrationRow>> {
    let row = conn
        .query_row(
            &format!(
                "SELECT {ANTHROPIC_CALIBRATION_COLUMNS} FROM anthropic_window_calibrations \
                 WHERE subject_id=?1 AND plan=?2 AND window_kind=?3"
            ),
            rusqlite::params![subject_id, plan, window_kind],
            sqlite_anthropic_calibration_row,
        )
        .optional()
        .context("load SQLite Anthropic calibration")?;
    if let Some(row) = &row {
        validate_anthropic_calibration_row(row)?;
    }
    Ok(row)
}

pub fn list_anthropic_calibrations(conn: &Connection) -> Result<Vec<AnthropicCalibrationRow>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {ANTHROPIC_CALIBRATION_COLUMNS} FROM anthropic_window_calibrations \
         ORDER BY subject_id,plan,window_kind"
    ))?;
    let rows = statement
        .query_map([], sqlite_anthropic_calibration_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for row in &rows {
        validate_anthropic_calibration_row(row)?;
    }
    Ok(rows)
}

pub fn load_anthropic_window_observations(
    conn: &Connection,
    subject_id: &str,
    plan: &str,
    window_kind: &str,
) -> Result<Vec<AnthropicWindowObservation>> {
    let mut statement = conn.prepare(
        "SELECT subject_id,plan,window_kind,window_duration_mins,resets_at,observed_at,\
           used_fraction_units,measurement_resolution_fraction_units,gateway_spend_nano,\
           observation_source,source_request_id FROM anthropic_window_observations \
         WHERE subject_id=?1 AND plan=?2 AND window_kind=?3 ORDER BY observed_at,id",
    )?;
    let rows = statement
        .query_map(rusqlite::params![subject_id, plan, window_kind], |row| {
            Ok(AnthropicWindowObservation {
                subject_id: row.get(0)?,
                plan: row.get(1)?,
                window_kind: row.get(2)?,
                window_duration_mins: row.get(3)?,
                resets_at: row.get(4)?,
                observed_at: row.get(5)?,
                used_fraction_units: row.get(6)?,
                measurement_resolution_fraction_units: row.get(7)?,
                gateway_spend_nano: row.get(8)?,
                observation_source: row.get(9)?,
                source_request_id: row.get(10)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for row in &rows {
        validate_anthropic_window_observation(row)?;
    }
    Ok(rows)
}

pub fn save_anthropic_calibration(
    conn: &Connection,
    state: &AnthropicCalibrationRow,
    observation: &AnthropicWindowObservation,
) -> Result<Option<i64>> {
    validate_anthropic_calibration_pair(state, observation)?;
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .context("begin SQLite Anthropic calibration CAS")?;
    let values = rusqlite::params![
        state.subject_id,
        state.plan,
        state.window_kind,
        state.window_duration_mins,
        state.resets_at,
        state.anchor_used_fraction_units,
        state.anchor_resolution_fraction_units,
        state.anchor_spend_nano,
        state.used_fraction_units,
        state.measurement_resolution_fraction_units,
        state.observed_at,
        state.observed_fraction_units,
        state.observed_spend_nano,
        state.samples,
        state.unattributed_fraction_units,
        state.current_capacity_nano,
        state.current_low_nano,
        state.current_high_nano,
        state.current_confidence_bp,
        state.last_measured_at,
        state.estimator_version,
        state.version,
        state.updated_ts,
    ];
    let changed = if state.version == 0 {
        tx.execute(
            "INSERT INTO anthropic_window_calibrations(\
               subject_id,plan,window_kind,window_duration_mins,resets_at,\
               anchor_used_fraction_units,anchor_resolution_fraction_units,anchor_spend_nano,\
               used_fraction_units,measurement_resolution_fraction_units,observed_at,\
               observed_fraction_units,observed_spend_nano,samples,unattributed_fraction_units,\
               current_capacity_nano,current_low_nano,current_high_nano,current_confidence_bp,\
               last_measured_at,estimator_version,version,updated_ts) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,\
                    ?18,?19,?20,?21,?22+1,?23) \
             ON CONFLICT(subject_id,plan,window_kind) DO NOTHING",
            values,
        )?
    } else {
        tx.execute(
            "UPDATE anthropic_window_calibrations SET \
               window_duration_mins=?4,resets_at=?5,anchor_used_fraction_units=?6,\
               anchor_resolution_fraction_units=?7,anchor_spend_nano=?8,\
               used_fraction_units=?9,measurement_resolution_fraction_units=?10,observed_at=?11,\
               observed_fraction_units=?12,observed_spend_nano=?13,samples=?14,\
               unattributed_fraction_units=?15,current_capacity_nano=?16,current_low_nano=?17,\
               current_high_nano=?18,current_confidence_bp=?19,last_measured_at=?20,\
               estimator_version=?21,version=version+1,updated_ts=?23 \
             WHERE subject_id=?1 AND plan=?2 AND window_kind=?3 AND version=?22",
            values,
        )?
    };
    if changed == 0 {
        return Ok(None);
    }
    tx.execute(
        "INSERT INTO anthropic_window_observations(\
           subject_id,plan,window_kind,window_duration_mins,resets_at,observed_at,\
           used_fraction_units,measurement_resolution_fraction_units,gateway_spend_nano,\
           observation_source,source_request_id) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT DO NOTHING",
        rusqlite::params![
            observation.subject_id,
            observation.plan,
            observation.window_kind,
            observation.window_duration_mins,
            observation.resets_at,
            observation.observed_at,
            observation.used_fraction_units,
            observation.measurement_resolution_fraction_units,
            observation.gateway_spend_nano,
            observation.observation_source,
            observation.source_request_id,
        ],
    )?;
    tx.commit()?;
    Ok(Some(state.version.saturating_add(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(request_id: &str, completed_at: i64) -> ProviderTurnCalibrationEvent {
        ProviderTurnCalibrationEvent {
            provider: PROVIDER_ANTHROPIC.to_owned(),
            request_id: request_id.to_owned(),
            subject_id: "operator@example.test".to_owned(),
            model_id: "claude-sonnet-4-5".to_owned(),
            service_tier: "standard".to_owned(),
            inference_geo: "global".to_owned(),
            tariff_schedule_id: "anthropic/test/v1".to_owned(),
            priced_ts: completed_at,
            completed_at,
            input_tokens: 1_000,
            audio_input_tokens: 0,
            cache_read_tokens: 200,
            cached_audio_input_tokens: 0,
            cache_write_5m_tokens: 50,
            cache_write_1h_tokens: 25,
            output_tokens: 500,
            thinking_output_tokens: 0,
            image_output_tokens: 0,
            tool_prompt_tokens: 0,
            search_queries: 0,
            grounded_search_prompts: 0,
            api_input_nanousd: 3_000,
            api_audio_input_nanousd: 0,
            api_cache_read_nanousd: 100,
            api_cached_audio_input_nanousd: 0,
            api_cache_write_5m_nanousd: 200,
            api_cache_write_1h_nanousd: 400,
            api_output_nanousd: 7_500,
            api_image_output_nanousd: 0,
            api_search_nanousd: 0,
            api_total_nanousd: 11_200,
        }
    }

    fn anchor(
        window_kind: &str,
        observed_at: i64,
    ) -> (AnthropicCalibrationRow, AnthropicWindowObservation) {
        let duration = if window_kind == "5h" { 300 } else { 10_080 };
        let observation = AnthropicWindowObservation {
            subject_id: "operator@example.test".to_owned(),
            plan: "max20".to_owned(),
            window_kind: window_kind.to_owned(),
            window_duration_mins: duration,
            resets_at: 2_000_000_000 + duration * 60,
            observed_at,
            used_fraction_units: 12_000_000,
            measurement_resolution_fraction_units: 100_000,
            gateway_spend_nano: 0,
            observation_source: "poll".to_owned(),
            source_request_id: None,
        };
        let state = AnthropicCalibrationRow {
            subject_id: observation.subject_id.clone(),
            plan: observation.plan.clone(),
            window_kind: observation.window_kind.clone(),
            window_duration_mins: observation.window_duration_mins,
            resets_at: observation.resets_at,
            anchor_used_fraction_units: observation.used_fraction_units,
            anchor_resolution_fraction_units: observation.measurement_resolution_fraction_units,
            anchor_spend_nano: observation.gateway_spend_nano,
            used_fraction_units: observation.used_fraction_units,
            measurement_resolution_fraction_units: observation
                .measurement_resolution_fraction_units,
            observed_at,
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
            version: 0,
            updated_ts: observed_at,
        };
        (state, observation)
    }

    #[test]
    fn exact_replay_advances_subject_spend_once_and_changed_payload_conflicts() {
        let connection = crate::open(":memory:").unwrap();
        let event = turn("request-1", 100);

        let inserted = record_provider_turn_calibration_event(&connection, &event).unwrap();
        assert!(inserted.inserted);
        assert_eq!(inserted.spent_nano, 11_200);

        let replay = record_provider_turn_calibration_event(&connection, &event).unwrap();
        assert!(!replay.inserted);
        assert_eq!(replay.spent_nano, 11_200);

        let mut conflicting = event.clone();
        conflicting.output_tokens += 1;
        let error = record_provider_turn_calibration_event(&connection, &conflicting).unwrap_err();
        assert!(is_provider_turn_calibration_replay_conflict(&error));
        assert_eq!(
            provider_calibration_subject_spend(&connection, PROVIDER_ANTHROPIC, &event.subject_id,)
                .unwrap()
                .spent_nano,
            11_200,
        );

        record_provider_turn_calibration_event(&connection, &turn("request-2", 101)).unwrap();
        let report = provider_turn_calibration_report(&connection, PROVIDER_ANTHROPIC).unwrap();
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].turns, 2);
        assert_eq!(report[0].api_total_nanousd, 22_400);
        assert_eq!(report[0].cache_read_tokens, 400);
    }

    #[test]
    fn five_hour_and_weekly_rows_and_histories_are_independent_and_idempotent() {
        let connection = crate::open(":memory:").unwrap();
        let (five_hour, five_hour_observation) = anchor("5h", 100);
        let (weekly, weekly_observation) = anchor("7d", 101);

        assert_eq!(
            save_anthropic_calibration(&connection, &five_hour, &five_hour_observation).unwrap(),
            Some(1),
        );
        assert_eq!(
            save_anthropic_calibration(&connection, &weekly, &weekly_observation).unwrap(),
            Some(1),
        );
        assert_eq!(list_anthropic_calibrations(&connection).unwrap().len(), 2);
        assert_eq!(
            load_anthropic_window_observations(
                &connection,
                "operator@example.test",
                "max20",
                "5h",
            )
            .unwrap()
            .len(),
            1,
        );
        assert_eq!(
            load_anthropic_window_observations(
                &connection,
                "operator@example.test",
                "max20",
                "7d",
            )
            .unwrap()
            .len(),
            1,
        );

        assert_eq!(
            save_anthropic_calibration(&connection, &five_hour, &five_hour_observation).unwrap(),
            None,
        );
        assert_eq!(
            load_anthropic_window_observations(
                &connection,
                "operator@example.test",
                "max20",
                "5h",
            )
            .unwrap()
            .len(),
            1,
        );
    }
}
