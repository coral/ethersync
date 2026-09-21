//! Thin browser binding; parsing and synchronization live in ethersync-protocol.
use ethersync_protocol::{
    CorrectionPolicy, SyncState, decode_snapshot,
    probes::Probes,
    timeline::{FollowerCore, Timeline, View},
};
use wasm_bindgen::prelude::*;

fn timestamp(ms: f64) -> Result<u64, JsValue> {
    if !ms.is_finite() || !(0.0..=9_223_372_036_854.0).contains(&ms) {
        return Err(JsValue::from_str(
            "timestamp must be finite nonnegative monotonic milliseconds within the signed nanosecond range",
        ));
    }
    Ok((ms * 1e6).round() as u64)
}
fn error(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

#[wasm_bindgen]
pub struct Follower {
    core: FollowerCore,
    probes: Probes,
    last_event: String,
}
#[wasm_bindgen]
impl Follower {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            core: FollowerCore::new(Timeline::default(), CorrectionPolicy::default()),
            probes: Probes::default(),
            last_event: String::new(),
        }
    }
    pub fn connecting(&mut self) {
        self.core.view.connection = ethersync_protocol::ConnectionState::Connecting;
    }
    pub fn connected(&mut self) {
        self.probes = Probes::default();
        self.core.connected();
    }
    pub fn disconnected(&mut self) {
        self.core.disconnected();
    }
    pub fn snapshot(&mut self, bytes: &[u8], now_ms: f64) -> Result<(), JsValue> {
        let now = timestamp(now_ms)?;
        let state = Timeline::from_wire(&decode_snapshot(bytes).map_err(error)?).map_err(error)?;
        if let Some(e) = self.core.state(state, now) {
            self.last_event = format!("{e:?}");
        }
        Ok(())
    }
    pub fn probe(&mut self, now_ms: f64) -> Result<Vec<u8>, JsValue> {
        self.probes.request(timestamp(now_ms)?).map_err(error)
    }
    pub fn probe_published(&mut self, now_ms: f64) -> Result<(), JsValue> {
        self.probes.publication_finished(timestamp(now_ms)?);
        Ok(())
    }
    pub fn reply(&mut self, bytes: &[u8], now_ms: f64) -> Result<(), JsValue> {
        self.reply_timed(bytes, now_ms, now_ms)
    }
    pub fn reply_timed(
        &mut self,
        bytes: &[u8],
        received_ms: f64,
        processed_ms: f64,
    ) -> Result<(), JsValue> {
        let received = timestamp(received_ms)?;
        let processed = timestamp(processed_ms)?;
        if processed < received {
            return Err(error("processing timestamp precedes receipt"));
        }
        if let Some(exchange) = self.probes.reply(bytes, received).map_err(error)?
            && let Some(e) =
                self.core
                    .measurement_timed(exchange, self.probes.last_publication_ns(), processed)
        {
            self.last_event = format!("{e:?}");
        }
        Ok(())
    }
    /// Last 128 matched exchanges. Decimal strings preserve all nanosecond timestamp bits.
    pub fn clock_trace(&self) -> Result<JsValue, JsValue> {
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Observation {
            t1: String,
            t2: String,
            t3: String,
            t4: String,
            accepted: bool,
            accepted_observations: String,
            publication_ns: Option<String>,
            processing_delay_ns: String,
            offset_ns: f64,
            drift_ppm: f64,
            evidence: Option<Evidence>,
        }
        let records: Vec<_> = self
            .core
            .estimator
            .trace()
            .map(|o| Observation {
                t1: o.exchange.t1.to_string(),
                t2: o.exchange.t2.to_string(),
                t3: o.exchange.t3.to_string(),
                t4: o.exchange.t4.to_string(),
                accepted: o.accepted,
                accepted_observations: o.mapping.accepted_observations.to_string(),
                publication_ns: o.publication_ns.map(|n| n.to_string()),
                processing_delay_ns: o.processing_delay_ns.to_string(),
                offset_ns: o.mapping.offset_ns,
                drift_ppm: o.mapping.drift * 1e6,
                evidence: o.mapping.evidence.map(Evidence::from),
            })
            .collect();
        serde_wasm_bindgen::to_value(&records).map_err(error)
    }
    /// Null/undefined when uninitialized or paused without a future control.
    pub fn next_boundary(&self, now_ms: f64) -> Result<JsValue, JsValue> {
        serialize_boundary(self.core.view, now_ms)
    }

    pub fn probe_interval_ms(&self) -> u32 {
        if self.core.view.sync == SyncState::Synchronized {
            250
        } else {
            50
        }
    }
    pub fn read(&mut self, now_ms: f64) -> Result<JsValue, JsValue> {
        self.read_for_presentation(now_ms, 0.)
    }
    /// Positive delay predicts ahead to presentation. Tick only at real now, never
    /// at the predicted time: future controls must not be latched prematurely.
    pub fn read_for_presentation(
        &mut self,
        now_ms: f64,
        compensation_delay_ms: f64,
    ) -> Result<JsValue, JsValue> {
        let now = timestamp(now_ms)?;
        let delay = timestamp(compensation_delay_ms)?;
        ethersync_protocol::timeline::presentation_time(now, delay).map_err(error)?;
        if let Some(e) = self.core.tick(now, 2_000_000_000) {
            self.last_event = format!("{e:?}");
        }
        serialize_reading(self.core.view, now, delay, &self.last_event)
    }
    /// Copy current state without ticking or refreshing it. Call read(nowMs) first
    /// when current staleness/lifecycle state is required. The snapshot owns its state.
    pub fn capture_snapshot(&self) -> TimecodeSnapshot {
        TimecodeSnapshot {
            view: self.core.view,
            event: self.last_event.clone(),
        }
    }
}
impl Default for Follower {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Evidence {
    lower_ms: f64,
    upper_ms: f64,
    samples: u32,
    consistent: bool,
}
impl From<ethersync_protocol::clock::OffsetEvidence> for Evidence {
    fn from(e: ethersync_protocol::clock::OffsetEvidence) -> Self {
        Self {
            lower_ms: e.lower_ns / 1e6,
            upper_ms: e.upper_ns / 1e6,
            samples: e.samples,
            consistent: e.consistent(),
        }
    }
}

/// Frozen timing state. Reads do not tick the follower or latch future controls.
#[wasm_bindgen]
pub struct TimecodeSnapshot {
    view: View,
    event: String,
}
#[wasm_bindgen]
impl TimecodeSnapshot {
    pub fn read(&self, now_ms: f64) -> Result<JsValue, JsValue> {
        self.read_for_presentation(now_ms, 0.)
    }
    pub fn read_for_presentation(&self, now_ms: f64, delay_ms: f64) -> Result<JsValue, JsValue> {
        serialize_reading(
            self.view,
            timestamp(now_ms)?,
            timestamp(delay_ms)?,
            &self.event,
        )
    }
    pub fn next_boundary(&self, now_ms: f64) -> Result<JsValue, JsValue> {
        serialize_boundary(self.view, now_ms)
    }
}

fn serialize_boundary(view: View, now_ms: f64) -> Result<JsValue, JsValue> {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Boundary {
        local_deadline_ms: f64,
        frames: String,
        subframe: u32,
        discontinuity: String,
        kind: String,
        uncertainty_ms: f64,
    }
    let b = view.next_boundary(timestamp(now_ms)?).map(|b| Boundary {
        local_deadline_ms: b.local_deadline_ns as f64 / 1e6,
        frames: b.position.frames.to_string(),
        subframe: b.position.subframe,
        discontinuity: b.discontinuity.to_string(),
        kind: format!("{:?}", b.kind),
        uncertainty_ms: b.uncertainty_ns / 1e6,
    });
    serde_wasm_bindgen::to_value(&b).map_err(error)
}

fn serialize_reading(view: View, now: u64, delay: u64, event: &str) -> Result<JsValue, JsValue> {
    let presentation =
        ethersync_protocol::timeline::presentation_time(now, delay).map_err(error)?;
    let r = view.evaluate_for_presentation(now, delay).map_err(error)?;
    let s = r.status;
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Reading<'a> {
        label: String,
        frames: f64,
        speed: f64,
        fps: f64,
        connection: String,
        synchronization: String,
        source: String,
        health: String,
        uncertainty_ms: f64,
        sample_age_ms: f64,
        offset_ms: f64,
        drift_ppm: f64,
        mapped_leader_ms: f64,
        correction_frames: f64,
        offset_evidence: Option<Evidence>,
        accepted_observations: String,
        discontinuity: String,
        event: &'a str,
    }
    serde_wasm_bindgen::to_value(&Reading {
        label: r.label().to_string(),
        frames: r.position.fixed() as f64 / 4294967296.,
        speed: r.rate.as_f64(),
        fps: r.format.fps(),
        connection: format!("{:?}", s.connection),
        synchronization: format!("{:?}", s.synchronization),
        source: format!("{:?}", s.source_kind),
        health: format!("{:?}", s.source_health),
        uncertainty_ms: s.uncertainty_ns / 1e6,
        sample_age_ms: s.sample_age_ns as f64 / 1e6,
        offset_ms: s.offset_ns / 1e6,
        drift_ppm: s.drift_ppm,
        mapped_leader_ms: view.mapping.leader_time(presentation) as f64 / 1e6,
        correction_frames: s.correction_frames,
        offset_evidence: s.offset_evidence.map(Evidence::from),
        accepted_observations: s.accepted_observations.to_string(),
        discontinuity: r.discontinuity.to_string(),
        event,
    })
    .map_err(error)
}
