//! Explicit source-time marker playback for application-owned animation requests.
//!
//! This is an engineering cursor over admitted text keys. The caller supplies
//! the source-time window, repeat policy and boundary delivery policy; sequence
//! frequency, cycle type and retail event meanings remain unapplied.
use super::{Entry, IntervalRequest, PreparedSequence, QueryLimits};
use crate::{Error, Result};
use serde::Serialize;

#[derive(Clone, Copy, Debug)]
pub struct SourceWindow {
    /// Explicit lower source-time coordinate; never inferred from frame count.
    pub start: f64,
    /// Explicit upper source-time coordinate; never inferred from NIF stop time.
    pub end: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepeatPolicy {
    Once,
    Loop,
}

/// Whether a caller wants keys exactly on the initial or repeated start boundary.
/// Text-key boundary behavior has not been measured against retail playback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoundaryDelivery {
    Emit,
    Skip,
}

#[derive(Clone, Copy, Debug)]
pub struct PlayRequest {
    pub expected_sha256: [u8; 32],
    pub sequence: u32,
    pub window: SourceWindow,
    pub repeat: RepeatPolicy,
    pub initial_boundary: BoundaryDelivery,
    pub loop_start_boundary: BoundaryDelivery,
}

#[derive(Clone, Copy, Debug)]
pub struct AdvanceLimits {
    /// Per-query bounds for the existing prepared text-key index.
    pub query: QueryLimits,
    /// Maximum markers retained in one start/advance result.
    pub max_events: usize,
    /// Maximum completed loop crossings in one advance call.
    pub max_loop_crossings: usize,
    /// Aggregate logical output and temporary query storage for one call.
    pub array_bytes: usize,
    /// Aggregate prepared-index query work for one call.
    pub work_units: usize,
    /// Prepared index, retained events and temporary query storage together.
    pub max_combined_retained_bytes: usize,
}
impl Default for AdvanceLimits {
    fn default() -> Self {
        Self {
            query: QueryLimits::default(),
            max_events: 4096,
            max_loop_crossings: 8,
            array_bytes: 16 * 1024 * 1024,
            work_units: 8_000_000,
            max_combined_retained_bytes: 48 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackState {
    Playing,
    Completed,
}

#[derive(Clone, Copy, Debug, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MarkerBoundary {
    Initial,
    Interval,
    WindowEnd,
    LoopStart,
}

#[derive(Debug, Serialize)]
pub struct MarkerEvent {
    pub generation: u64,
    pub cycle_index: u64,
    pub source_key_ordinal: usize,
    pub time_bits: u32,
    pub string_index: u32,
    /// Exact authored bytes; sound/action vocabulary is not interpreted here.
    pub raw_string_bytes: Vec<u8>,
    pub boundary: MarkerBoundary,
}

#[derive(Debug, Serialize)]
pub struct StartResult {
    pub generation: u64,
    pub interrupted_generation: Option<u64>,
    pub source_sha256: String,
    pub sequence: u32,
    pub source_time_bits: u64,
    pub events: Vec<MarkerEvent>,
    pub retail_behavior_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct AdvanceResult {
    pub generation: u64,
    pub source_time_before_bits: u64,
    pub source_time_after_bits: u64,
    pub source_delta_bits: u64,
    /// Once playback reports any source-time delta beyond its end here.
    pub unused_source_delta_bits: u64,
    pub cycles_crossed: u64,
    pub cycles_completed: u64,
    pub state: PlaybackState,
    pub events: Vec<MarkerEvent>,
    pub sequence_clock_fields_applied: bool,
    pub retail_behavior_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct CancelResult {
    pub generation: u64,
    pub source_sha256: String,
    pub sequence: u32,
    pub source_time_bits: u64,
    pub state_before_cancel: PlaybackState,
}

#[derive(Clone, Copy, Debug)]
struct Session {
    generation: u64,
    request: PlayRequest,
    position: f64,
    cycles_completed: u64,
    state: PlaybackState,
}

/// Serial application request owner for one explicitly bound sequence.
/// Starting another request interrupts the prior active request immediately;
/// no blend or retail transition duration is inferred.
#[derive(Debug)]
pub struct PlaybackController {
    next_generation: u64,
    active: Option<Session>,
}
impl Default for PlaybackController {
    fn default() -> Self {
        Self::new()
    }
}
impl PlaybackController {
    pub fn new() -> Self {
        Self {
            next_generation: 1,
            active: None,
        }
    }

    /// Validate the exact prepared source before replacing the current request.
    /// A failed start leaves the old request and generation unchanged.
    pub fn start(
        &mut self,
        prepared: &PreparedSequence,
        request: PlayRequest,
        limits: AdvanceLimits,
    ) -> Result<StartResult> {
        validate_request(prepared, request)?;
        let generation = self.next_generation;
        let next_generation = generation
            .checked_add(1)
            .ok_or_else(|| fail("request generation exhausted"))?;
        let mut budget = EventBudget::new(prepared, limits);
        let mut events = Vec::new();
        if request.initial_boundary == BoundaryDelivery::Emit {
            budget.point(
                prepared,
                request,
                generation,
                0,
                request.window.start,
                MarkerBoundary::Initial,
                &mut events,
            )?;
        }
        let interrupted_generation = self
            .active
            .filter(|session| session.state == PlaybackState::Playing)
            .map(|session| session.generation);
        self.active = Some(Session {
            generation,
            request,
            position: request.window.start,
            cycles_completed: 0,
            state: PlaybackState::Playing,
        });
        self.next_generation = next_generation;
        Ok(StartResult {
            generation,
            interrupted_generation,
            source_sha256: prepared.source_sha256().to_owned(),
            sequence: request.sequence,
            source_time_bits: request.window.start.to_bits(),
            events,
            retail_behavior_verified: false,
        })
    }

    /// Advance in explicit source-time units. Rendering frame count is absent.
    /// Each interval is `(previous, next]`; explicit initial/loop-start policies
    /// decide whether a key exactly at those boundaries is emitted.
    pub fn advance_by_source_delta(
        &mut self,
        expected_generation: u64,
        prepared: &PreparedSequence,
        source_delta: f64,
        limits: AdvanceLimits,
    ) -> Result<AdvanceResult> {
        let session = self
            .active
            .as_mut()
            .ok_or_else(|| fail("no active request"))?;
        if session.generation != expected_generation {
            return Err(fail("stale request generation"));
        }
        if session.state != PlaybackState::Playing {
            return Err(fail("request is already complete"));
        }
        validate_request(prepared, session.request)?;
        if !source_delta.is_finite() || source_delta < 0.0 {
            return Err(fail("source-time delta must be finite and nonnegative"));
        }

        // Build the entire result against local cursor values. Any identity,
        // work, event or storage refusal leaves the live request unchanged.
        let before = session.position;
        let mut position = before;
        let mut cycles_completed = session.cycles_completed;
        let mut cycles_crossed = 0u64;
        let mut remaining = source_delta;
        let mut unused = 0.0;
        let mut state = session.state;
        let mut events = Vec::new();
        let mut budget = EventBudget::new(prepared, limits);

        while remaining > 0.0 && state == PlaybackState::Playing {
            let distance_to_end = session.request.window.end - position;
            if distance_to_end <= 0.0 {
                // Loop requests are normalized to their explicit start at each
                // end boundary; reaching here means a malformed internal state.
                return Err(fail("cursor is outside the explicit source window"));
            }
            if session.request.repeat == RepeatPolicy::Once {
                let consumed = remaining.min(distance_to_end);
                let next = position + consumed;
                if consumed > 0.0 && next <= position {
                    return Err(fail("source-time delta is below coordinate precision"));
                }
                if consumed > 0.0 {
                    budget.range(
                        prepared,
                        session.request,
                        session.generation,
                        cycles_completed,
                        position,
                        next,
                        &mut events,
                    )?;
                }
                position = next;
                remaining -= consumed;
                if position >= session.request.window.end {
                    position = session.request.window.end;
                    state = PlaybackState::Completed;
                    unused = remaining;
                    remaining = 0.0;
                } else {
                    remaining = 0.0;
                }
            } else if remaining < distance_to_end {
                let next = position + remaining;
                if next <= position {
                    return Err(fail("source-time delta is below coordinate precision"));
                }
                budget.range(
                    prepared,
                    session.request,
                    session.generation,
                    cycles_completed,
                    position,
                    next,
                    &mut events,
                )?;
                position = next;
                remaining = 0.0;
            } else {
                // Include the authored final key at the closed upper boundary,
                // then enter a new cycle exactly once.
                budget.range(
                    prepared,
                    session.request,
                    session.generation,
                    cycles_completed,
                    position,
                    session.request.window.end,
                    &mut events,
                )?;
                remaining -= distance_to_end;
                cycles_completed = cycles_completed
                    .checked_add(1)
                    .ok_or_else(|| fail("loop counter overflow"))?;
                cycles_crossed = cycles_crossed
                    .checked_add(1)
                    .ok_or_else(|| fail("loop crossing counter overflow"))?;
                if cycles_crossed as usize > limits.max_loop_crossings {
                    return Err(fail("loop crossing budget exceeded"));
                }
                position = session.request.window.start;
                if session.request.loop_start_boundary == BoundaryDelivery::Emit {
                    budget.point(
                        prepared,
                        session.request,
                        session.generation,
                        cycles_completed,
                        position,
                        MarkerBoundary::LoopStart,
                        &mut events,
                    )?;
                }
            }
        }

        session.position = position;
        session.cycles_completed = cycles_completed;
        session.state = state;
        Ok(AdvanceResult {
            generation: session.generation,
            source_time_before_bits: before.to_bits(),
            source_time_after_bits: position.to_bits(),
            source_delta_bits: source_delta.to_bits(),
            unused_source_delta_bits: unused.to_bits(),
            cycles_crossed,
            cycles_completed,
            state,
            events,
            sequence_clock_fields_applied: false,
            retail_behavior_verified: false,
        })
    }

    /// Cancel only the exact live generation. No pending marker is synthesized.
    pub fn cancel(&mut self, expected_generation: u64) -> Result<CancelResult> {
        let session = self.active.ok_or_else(|| fail("no active request"))?;
        if session.generation != expected_generation {
            return Err(fail("stale request generation"));
        }
        self.active = None;
        Ok(CancelResult {
            generation: session.generation,
            source_sha256: hex_digest(&session.request.expected_sha256),
            sequence: session.request.sequence,
            source_time_bits: session.position.to_bits(),
            state_before_cancel: session.state,
        })
    }

    pub fn active_generation(&self) -> Option<u64> {
        self.active.map(|session| session.generation)
    }
}

struct EventBudget {
    limits: AdvanceLimits,
    prepared_bytes: usize,
    stored_event_bytes: usize,
    work_units: usize,
}
impl EventBudget {
    fn new(prepared: &PreparedSequence, limits: AdvanceLimits) -> Self {
        Self {
            limits,
            prepared_bytes: prepared.usage().retained_bytes,
            stored_event_bytes: 0,
            work_units: 0,
        }
    }

    fn point(
        &mut self,
        prepared: &PreparedSequence,
        request: PlayRequest,
        generation: u64,
        cycle_index: u64,
        time: f64,
        boundary: MarkerBoundary,
        out: &mut Vec<MarkerEvent>,
    ) -> Result<()> {
        self.query(
            prepared,
            request,
            generation,
            cycle_index,
            time,
            time,
            true,
            Some(boundary),
            out,
        )
    }

    fn range(
        &mut self,
        prepared: &PreparedSequence,
        request: PlayRequest,
        generation: u64,
        cycle_index: u64,
        start: f64,
        end: f64,
        out: &mut Vec<MarkerEvent>,
    ) -> Result<()> {
        self.query(
            prepared,
            request,
            generation,
            cycle_index,
            start,
            end,
            false,
            None,
            out,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn query(
        &mut self,
        prepared: &PreparedSequence,
        request: PlayRequest,
        generation: u64,
        cycle_index: u64,
        start: f64,
        end: f64,
        include_start: bool,
        point_boundary: Option<MarkerBoundary>,
        out: &mut Vec<MarkerEvent>,
    ) -> Result<()> {
        let array_left = self
            .limits
            .array_bytes
            .checked_sub(self.stored_event_bytes)
            .ok_or_else(|| fail("event output storage budget exceeded"))?;
        let combined_left = self
            .limits
            .max_combined_retained_bytes
            .checked_sub(self.stored_event_bytes)
            .ok_or_else(|| fail("event combined storage budget exceeded"))?;
        let work_left = self
            .limits
            .work_units
            .checked_sub(self.work_units)
            .ok_or_else(|| fail("event work budget exceeded"))?;
        let query_limits = QueryLimits {
            entries: self.limits.query.entries,
            array_bytes: self.limits.query.array_bytes.min(array_left),
            work_units: self.limits.query.work_units.min(work_left),
            max_combined_retained_bytes: self
                .limits
                .query
                .max_combined_retained_bytes
                .min(combined_left),
        };
        let result = prepared.query(
            IntervalRequest {
                expected_sha256: request.expected_sha256,
                sequence: request.sequence,
                source_start: start,
                source_end: end,
            },
            query_limits,
        )?;
        let query_bytes = result.usage.charged_bytes;
        let concurrent = self
            .stored_event_bytes
            .checked_add(query_bytes)
            .ok_or_else(|| fail("event query storage accounting overflow"))?;
        if concurrent > self.limits.array_bytes {
            return Err(fail("event/query array storage budget exceeded"));
        }
        let combined = self
            .prepared_bytes
            .checked_add(concurrent)
            .ok_or_else(|| fail("event combined storage accounting overflow"))?;
        if combined > self.limits.max_combined_retained_bytes {
            return Err(fail("event/prepared combined storage budget exceeded"));
        }
        self.work_units = self
            .work_units
            .checked_add(result.usage.work_units)
            .filter(|used| *used <= self.limits.work_units)
            .ok_or_else(|| fail("event work budget exceeded"))?;

        for entry in result.observation.entries {
            let source_time = f64::from(f32::from_bits(entry.time_bits));
            if !include_start && source_time <= start {
                continue;
            }
            if source_time > end {
                continue;
            }
            if out.len() >= self.limits.max_events {
                return Err(fail("event count budget exceeded"));
            }
            let boundary = point_boundary.unwrap_or_else(|| {
                if source_time == request.window.end {
                    MarkerBoundary::WindowEnd
                } else {
                    MarkerBoundary::Interval
                }
            });
            let event_bytes = std::mem::size_of::<MarkerEvent>()
                .checked_add(entry.raw_string_bytes.len())
                .ok_or_else(|| fail("event storage accounting overflow"))?;
            self.stored_event_bytes = self
                .stored_event_bytes
                .checked_add(event_bytes)
                .filter(|used| *used <= self.limits.array_bytes)
                .ok_or_else(|| fail("event output storage budget exceeded"))?;
            let total = self
                .prepared_bytes
                .checked_add(self.stored_event_bytes)
                .ok_or_else(|| fail("event combined storage accounting overflow"))?;
            if total > self.limits.max_combined_retained_bytes {
                return Err(fail("event/prepared combined storage budget exceeded"));
            }
            out.push(event(entry, generation, cycle_index, boundary));
        }
        Ok(())
    }
}

fn event(entry: Entry, generation: u64, cycle_index: u64, boundary: MarkerBoundary) -> MarkerEvent {
    MarkerEvent {
        generation,
        cycle_index,
        source_key_ordinal: entry.source_key_ordinal,
        time_bits: entry.time_bits,
        string_index: entry.string_index,
        raw_string_bytes: entry.raw_string_bytes,
        boundary,
    }
}

fn validate_request(prepared: &PreparedSequence, request: PlayRequest) -> Result<()> {
    if prepared.sequence() != request.sequence {
        return Err(fail("prepared sequence differs from request"));
    }
    if prepared.source_sha256() != hex_digest(&request.expected_sha256) {
        return Err(fail("prepared source SHA256 differs from request"));
    }
    if !request.window.start.is_finite()
        || !request.window.end.is_finite()
        || request.window.start >= request.window.end
    {
        return Err(fail("source window must have finite increasing bounds"));
    }
    Ok(())
}

fn hex_digest(digest: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    for byte in digest {
        value.push(char::from(HEX[(byte >> 4) as usize]));
        value.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    value
}

fn fail(detail: &str) -> Error {
    Error::Unsupported(format!("animation playback: {detail}"))
}
