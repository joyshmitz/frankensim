//! The power-quality analyser of the generator breaker cabinet.
//!
//! A measuring instrument: it reads the three phase voltages of the bus
//! after every accepted sub-step and keeps its own record of voltage events,
//! stamped with its own clock. It switches nothing and feeds nothing back
//! into the plant, so it is not a Component of the system: it is display
//! over computed and prescribed quantities. Its other read-outs (phase
//! voltages and currents, power, frequency, unbalance) are the snapshot's.
//!
//! An event on a phase opens in the sub-step in which the voltage is first
//! past a threshold and is recorded when the voltage has come back past that
//! threshold by the hysteresis. Each kind has its own threshold and is
//! followed by itself: a dip lasts while the voltage is below the dip level,
//! an interruption while it is below the interruption level, so a voltage
//! that falls to nothing and recovers in stages leaves two records, a dip
//! for the whole excursion (its residual voltage the lowest reached) and an
//! interruption for the part of it spent without voltage. Resolution is the
//! plant's sub-step; nothing shorter exists for it.
//!
//! The record holds a limited number of closed events. A further one
//! displaces the oldest, and the analyser counts what it has displaced.

use crate::cards::PowerQualityCard;

/// Kinds of [`QualityEvent`].
pub mod quality_kind {
    /// Voltage dip: residual voltage below the dip threshold.
    pub const DIP: u32 = 1;
    /// Voltage swell: voltage above the swell threshold.
    pub const SWELL: u32 = 2;
    /// Interruption: residual voltage below the interruption threshold.
    pub const INTERRUPTION: u32 = 3;
}

/// Number of values per event in the flat export.
pub const QUALITY_EVENT_STRIDE: usize = 5;

/// One recorded voltage event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QualityEvent {
    /// Kind, see [`quality_kind`].
    pub kind: u32,
    /// Phase: 1, 2, 3 for a, b, c.
    pub phase: u32,
    /// Start by the analyser's own clock [s].
    pub start_s: f64,
    /// Duration [s].
    pub duration_s: f64,
    /// Residual (lowest) voltage of a dip or interruption, highest voltage
    /// of a swell [V, phase to neutral].
    pub extreme_v: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct OpenEvent {
    start_s: f64,
    duration_s: f64,
    extreme_pu: f64,
}

/// The analyser: thresholds, its clock's offset, the events still open and
/// the record of closed ones.
#[derive(Debug, Clone, PartialEq)]
pub struct PowerQualityAnalyser {
    card: PowerQualityCard,
    rated_phase_voltage_v: f64,
    clock_offset_s: f64,
    /// Open dip per phase.
    low: [Option<OpenEvent>; 3],
    /// Open interruption per phase.
    lost: [Option<OpenEvent>; 3],
    /// Open swell per phase.
    high: [Option<OpenEvent>; 3],
    record: Vec<QualityEvent>,
    recorded: u32,
    displaced: u32,
}

/// One threshold followed on one phase: open past `opens`, closed when back
/// past `closes`. `below` says which side is the event's.
fn follow(
    open: &mut Option<OpenEvent>,
    voltage: f64,
    below: bool,
    (opens, closes): (f64, f64),
    now: f64,
    dt_s: f64,
) -> Option<OpenEvent> {
    let past = |level: f64| if below { voltage < level } else { voltage > level };
    match *open {
        None if past(opens) => {
            *open = Some(OpenEvent {
                start_s: now,
                duration_s: dt_s,
                extreme_pu: voltage,
            });
            None
        }
        None => None,
        // Still past the closing level (inside the hysteresis counts): the same event.
        Some(event) if past(closes) => {
            *open = Some(OpenEvent {
                start_s: event.start_s,
                duration_s: event.duration_s + dt_s,
                extreme_pu: if below {
                    event.extreme_pu.min(voltage)
                } else {
                    event.extreme_pu.max(voltage)
                },
            });
            None
        }
        Some(event) => {
            *open = None;
            Some(event)
        }
    }
}

impl PowerQualityAnalyser {
    /// An analyser with the given thresholds on a system of
    /// `rated_phase_voltage_v`, its clock in step with the model's.
    #[must_use]
    pub fn new(card: PowerQualityCard, rated_phase_voltage_v: f64) -> Self {
        Self {
            card,
            rated_phase_voltage_v,
            clock_offset_s: 0.0,
            low: [None; 3],
            lost: [None; 3],
            high: [None; 3],
            record: Vec::new(),
            recorded: 0,
            displaced: 0,
        }
    }

    /// The thresholds.
    #[must_use]
    pub fn card(&self) -> &PowerQualityCard {
        &self.card
    }

    /// Set how far the analyser's clock is ahead of the model's time [s].
    pub fn set_clock_offset_s(&mut self, offset_s: f64) {
        self.clock_offset_s = offset_s;
    }

    /// The analyser's clock at model time `model_time_s` [s].
    #[must_use]
    pub fn clock_s(&self, model_time_s: f64) -> f64 {
        model_time_s + self.clock_offset_s
    }

    /// Read the phase voltages [pu] that held over the sub-step from
    /// `model_time_s` lasting `dt_s`.
    pub fn observe(&mut self, model_time_s: f64, dt_s: f64, phase_voltage_pu: [f64; 3]) {
        let now = self.clock_s(model_time_s);
        let card = self.card;
        let hysteresis = card.hysteresis_pu;
        for phase in 0..3 {
            let voltage = phase_voltage_pu[phase];
            // Each kind against its own level, closed past that level by the hysteresis.
            let dip = (card.dip_pu, card.dip_pu + hysteresis);
            if let Some(event) = follow(&mut self.low[phase], voltage, true, dip, now, dt_s) {
                self.close(quality_kind::DIP, phase, event);
            }
            let interruption = (card.interruption_pu, card.interruption_pu + hysteresis);
            if let Some(event) =
                follow(&mut self.lost[phase], voltage, true, interruption, now, dt_s)
            {
                self.close(quality_kind::INTERRUPTION, phase, event);
            }
            let swell = (card.swell_pu, card.swell_pu - hysteresis);
            if let Some(event) = follow(&mut self.high[phase], voltage, false, swell, now, dt_s) {
                self.close(quality_kind::SWELL, phase, event);
            }
        }
    }

    fn close(&mut self, kind: u32, phase: usize, open: OpenEvent) {
        if self.card.record_capacity == 0 {
            self.displaced += 1;
            self.recorded += 1;
            return;
        }
        if self.record.len() >= self.card.record_capacity as usize {
            self.record.remove(0);
            self.displaced += 1;
        }
        self.record.push(QualityEvent {
            kind,
            phase: phase as u32 + 1,
            start_s: open.start_s,
            duration_s: open.duration_s,
            extreme_v: open.extreme_pu * self.rated_phase_voltage_v,
        });
        self.recorded += 1;
    }

    /// The record of closed events, oldest first.
    #[must_use]
    pub fn events(&self) -> &[QualityEvent] {
        &self.record
    }

    /// Empty the record. Events still open stay open.
    pub fn clear_events(&mut self) {
        self.record.clear();
    }

    /// Number of events open now.
    #[must_use]
    pub fn open_events(&self) -> u32 {
        (self.low.iter().flatten().count()
            + self.lost.iter().flatten().count()
            + self.high.iter().flatten().count()) as u32
    }

    /// Closed events the record has displaced since the model started: they
    /// were recorded and are no longer in it.
    #[must_use]
    pub fn displaced_events(&self) -> u32 {
        self.displaced
    }

    /// Number of events recorded since the model started.
    #[must_use]
    pub fn recorded_events(&self) -> u32 {
        self.recorded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dip_a_swell_and_an_interruption_are_recorded_once_each_with_their_extremes() {
        let mut analyser = PowerQualityAnalyser::new(PowerQualityCard::DEFAULT, 230.0);
        analyser.set_clock_offset_s(100.0);
        let dt = 0.01;
        let mut time = 0.0;
        let mut run = |analyser: &mut PowerQualityAnalyser, steps: usize, voltage: [f64; 3]| {
            for _ in 0..steps {
                analyser.observe(time, dt, voltage);
                time += dt;
            }
        };
        run(&mut analyser, 50, [1.0, 1.0, 1.0]);
        assert_eq!(analyser.open_events(), 0);
        // Phase b dips to 0.73 for 20 steps, with one lower step inside.
        run(&mut analyser, 10, [1.0, 0.73, 1.0]);
        run(&mut analyser, 1, [1.0, 0.70, 1.0]);
        run(&mut analyser, 9, [1.0, 0.73, 1.0]);
        assert_eq!(analyser.open_events(), 1);
        assert!(analyser.events().is_empty(), "an open event is not in the record");
        // Inside the hysteresis it is still the same event.
        run(&mut analyser, 5, [1.0, 0.91, 1.0]);
        assert_eq!(analyser.open_events(), 1);
        run(&mut analyser, 5, [1.0, 1.0, 1.0]);
        // Phase c swells for 3 steps; phase a is interrupted for 7.
        run(&mut analyser, 3, [1.0, 1.0, 1.15]);
        run(&mut analyser, 7, [0.01, 1.0, 1.0]);
        run(&mut analyser, 5, [1.0, 1.0, 1.0]);
        let events = analyser.events();
        // The phase without voltage leaves two records: a dip and an interruption.
        assert_eq!(events.len(), 4);
        let dip = events[0];
        assert_eq!((dip.kind, dip.phase), (quality_kind::DIP, 2));
        assert!((dip.start_s - 100.5).abs() < 1.0e-9, "start {}", dip.start_s);
        assert!((dip.duration_s - 0.25).abs() < 1.0e-9, "duration {}", dip.duration_s);
        assert!((dip.extreme_v - 0.70 * 230.0).abs() < 1.0e-9);
        let swell = events[1];
        assert_eq!((swell.kind, swell.phase), (quality_kind::SWELL, 3));
        assert!((swell.duration_s - 0.03).abs() < 1.0e-9);
        assert!((swell.extreme_v - 1.15 * 230.0).abs() < 1.0e-9);
        let deep = events[2];
        assert_eq!((deep.kind, deep.phase), (quality_kind::DIP, 1));
        assert!((deep.extreme_v - 0.01 * 230.0).abs() < 1.0e-9);
        let interruption = events[3];
        assert_eq!((interruption.kind, interruption.phase), (quality_kind::INTERRUPTION, 1));
        assert!((interruption.duration_s - 0.07).abs() < 1.0e-9);
        assert_eq!(analyser.recorded_events(), 4);
        assert_eq!(analyser.open_events(), 0);
        assert_eq!(analyser.displaced_events(), 0);
    }

    #[test]
    fn an_interruption_ends_when_the_voltage_is_back_above_its_own_level() {
        // No voltage for 10 steps, then half voltage for 200, then 0.91 for 200, then full.
        let mut analyser = PowerQualityAnalyser::new(PowerQualityCard::DEFAULT, 230.0);
        let dt = 0.01;
        let mut time = 0.0;
        let mut run = |analyser: &mut PowerQualityAnalyser, steps: usize, voltage: f64| {
            for _ in 0..steps {
                analyser.observe(time, dt, [voltage, 1.0, 1.0]);
                time += dt;
            }
        };
        run(&mut analyser, 10, 0.0);
        assert_eq!(analyser.open_events(), 2, "a dip and an interruption are open");
        run(&mut analyser, 200, 0.5);
        // Half voltage is not an interruption any more: that record is closed, at the
        // time spent without voltage. The dip goes on.
        assert_eq!(analyser.open_events(), 1);
        let interruption = analyser.events()[0];
        assert_eq!(interruption.kind, quality_kind::INTERRUPTION);
        assert!((interruption.duration_s - 0.10).abs() < 1.0e-9, "{}", interruption.duration_s);
        run(&mut analyser, 200, 0.91);
        assert_eq!(analyser.open_events(), 1, "inside the dip's hysteresis");
        run(&mut analyser, 10, 1.0);
        let dip = analyser.events()[1];
        assert_eq!(dip.kind, quality_kind::DIP);
        assert!((dip.duration_s - 4.10).abs() < 1.0e-9, "{}", dip.duration_s);
        assert_eq!(dip.extreme_v, 0.0);
    }

    #[test]
    fn a_full_record_displaces_its_oldest_event_and_counts_it() {
        let card = PowerQualityCard {
            record_capacity: 3,
            ..PowerQualityCard::DEFAULT
        };
        let mut analyser = PowerQualityAnalyser::new(card, 230.0);
        for event in 0..5 {
            let start = f64::from(event);
            analyser.observe(start, 0.5, [1.0, 0.8, 1.0]);
            analyser.observe(start + 0.5, 0.5, [1.0; 3]);
            assert_eq!(analyser.displaced_events(), u32::try_from((event - 2).max(0)).unwrap());
        }
        let starts: Vec<f64> = analyser.events().iter().map(|event| event.start_s).collect();
        assert_eq!(starts, [2.0, 3.0, 4.0]);
        assert_eq!(analyser.recorded_events(), 5);
    }
}
