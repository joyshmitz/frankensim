//! The revenue meter of the generator breaker cabinet.
//!
//! A second measuring instrument beside the power-quality analyser, and like
//! it not a Component: it reads computed and prescribed quantities after
//! every accepted sub-step, switches nothing and feeds nothing back.
//!
//! It does two things.
//!
//! **Energy.** Four registers: active energy exported and imported, reactive
//! energy exported and imported. The active registers take the energy the
//! kernel's ledger booked at the generator's bus port over each sub-step, by
//! its sign; the reactive ones take the generator's reactive power times the
//! sub-step. The meter's accuracy class is not modelled: its energy is the
//! engine's.
//!
//! **Voltage supervision.** Once per supervision period of its own clock the
//! meter looks at the three phase voltages as they are at that instant, and
//! at nothing in between. A phase below the low level, above the high level
//! or below the loss level at such an instant opens a record in the journal
//! of that kind; the record is closed by the first later instant at which no
//! phase is in that condition. So a record holds two readings of the meter's
//! clock and the phases seen: no depth, no highest value, and no duration
//! finer than the period. A disturbance that begins and ends between two
//! instants leaves no record at all. None of this is a rule about durations:
//! it is what looking once per period gives.
//!
//! The journals are of limited capacity. What a full one does with the next
//! record is a card value.

use std::collections::VecDeque;

use crate::cards::{JournalOverflow, MeterCard};

/// The meter's journals; also the bit of each in the overflow flags
/// (`1 << (journal - 1)`).
pub mod journal {
    /// A phase below the low-voltage level.
    pub const LOW_VOLTAGE: u32 = 1;
    /// A phase above the high-voltage level.
    pub const HIGH_VOLTAGE: u32 = 2;
    /// A phase below the loss-of-voltage level.
    pub const LOSS_OF_VOLTAGE: u32 = 3;
    /// Number of journals.
    pub const COUNT: usize = 3;
}

/// Number of values per record in the flat export.
pub const METER_EVENT_STRIDE: usize = 4;

/// One record of a journal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeterRecord {
    /// Which journal, see [`journal`].
    pub journal: u32,
    /// Phases seen in the condition at the supervision instants of this
    /// record: bit 0 phase a, bit 1 phase b, bit 2 phase c.
    pub phase_bits: u32,
    /// The supervision instant that first saw the condition, by the meter's
    /// own clock [s]. Not the start of the disturbance.
    pub seen_s: f64,
    /// The first later supervision instant that saw no phase in the
    /// condition, by the meter's clock [s]; NaN until then.
    pub cleared_s: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct Journal {
    capacity: usize,
    records: VecDeque<MeterRecord>,
    /// The condition was present at the last supervision instant.
    present: bool,
    /// The record of the present condition is the journal's newest one.
    recorded: bool,
    /// A record was displaced or not written since the journal was cleared.
    overflowed: bool,
}

impl Journal {
    fn new(capacity: u32) -> Self {
        Self {
            capacity: capacity as usize,
            records: VecDeque::new(),
            present: false,
            recorded: false,
            overflowed: false,
        }
    }
}

/// The meter: its card, its clock, its registers and its journals.
#[derive(Debug, Clone, PartialEq)]
pub struct RevenueMeter {
    card: MeterCard,
    clock_offset_s: f64,
    sample_phase_s: f64,
    active_export_j: f64,
    active_import_j: f64,
    reactive_export_var_s: f64,
    reactive_import_var_s: f64,
    journals: [Journal; journal::COUNT],
}

impl RevenueMeter {
    /// A meter with empty registers and journals, its clock in step with the
    /// model's and its supervision instants on the whole periods.
    #[must_use]
    pub fn new(card: MeterCard) -> Self {
        Self {
            card,
            clock_offset_s: 0.0,
            sample_phase_s: 0.0,
            active_export_j: 0.0,
            active_import_j: 0.0,
            reactive_export_var_s: 0.0,
            reactive_import_var_s: 0.0,
            journals: [
                Journal::new(card.low_voltage_capacity),
                Journal::new(card.high_voltage_capacity),
                Journal::new(card.loss_of_voltage_capacity),
            ],
        }
    }

    /// The card.
    #[must_use]
    pub fn card(&self) -> &MeterCard {
        &self.card
    }

    /// Set how far the meter's clock is ahead of the model's time [s].
    pub fn set_clock_offset_s(&mut self, offset_s: f64) {
        self.clock_offset_s = offset_s;
    }

    /// Set where within its period the meter supervises the voltage: the
    /// instants are at `n * period + phase` of the meter's own clock. Taken
    /// modulo the period.
    pub fn set_sample_phase_s(&mut self, phase_s: f64) {
        self.sample_phase_s = phase_s.rem_euclid(self.card.supervision_period_s);
    }

    /// Where within its period the meter supervises the voltage [s].
    #[must_use]
    pub fn sample_phase_s(&self) -> f64 {
        self.sample_phase_s
    }

    /// The meter's clock at model time `model_time_s` [s].
    #[must_use]
    pub fn clock_s(&self, model_time_s: f64) -> f64 {
        model_time_s + self.clock_offset_s
    }

    /// Read one accepted sub-step that began at `model_time_s` and lasted
    /// `dt_s`: the phase voltages [pu] that held over it, the energy that
    /// left through the generator's bus port [J] (negative when it entered)
    /// and the reactive power delivered [var] (negative when absorbed).
    pub fn observe(
        &mut self,
        model_time_s: f64,
        dt_s: f64,
        phase_voltage_pu: [f64; 3],
        exported_j: f64,
        reactive_var: f64,
    ) {
        if exported_j >= 0.0 {
            self.active_export_j += exported_j;
        } else {
            self.active_import_j -= exported_j;
        }
        let reactive_var_s = reactive_var * dt_s;
        if reactive_var_s >= 0.0 {
            self.reactive_export_var_s += reactive_var_s;
        } else {
            self.reactive_import_var_s -= reactive_var_s;
        }

        // Supervision instants of the meter's clock inside this sub-step. A sub-step owns
        // the instants from its start to just short of its end; the tolerance only decides
        // to which of two adjoining sub-steps an instant on their common boundary belongs
        // when the two times differ in their last digits.
        let period = self.card.supervision_period_s;
        let tolerance = 1.0e-6 * period;
        let start = self.clock_s(model_time_s);
        let end = start + dt_s;
        let mut index = ((start - tolerance - self.sample_phase_s) / period).ceil();
        loop {
            let instant = index * period + self.sample_phase_s;
            if instant >= end - tolerance {
                break;
            }
            self.supervise(instant, phase_voltage_pu);
            index += 1.0;
        }
    }

    /// One look at the phase voltages, at `instant_s` of the meter's clock.
    fn supervise(&mut self, instant_s: f64, phase_voltage_pu: [f64; 3]) {
        let card = self.card;
        let mut seen = [0_u32; journal::COUNT];
        for (phase, voltage) in phase_voltage_pu.into_iter().enumerate() {
            let bit = 1 << phase;
            if voltage < card.loss_of_voltage_pu {
                seen[2] |= bit;
            } else if voltage < card.low_voltage_pu {
                seen[0] |= bit;
            } else if voltage > card.high_voltage_pu {
                seen[1] |= bit;
            }
        }
        for (index, journal) in self.journals.iter_mut().enumerate() {
            let phase_bits = seen[index];
            if phase_bits != 0 {
                if !journal.present {
                    journal.present = true;
                    let full = journal.records.len() >= journal.capacity;
                    if full {
                        journal.overflowed = true;
                    }
                    let write = !full
                        || (card.overflow == JournalOverflow::OverwriteOldest
                            && journal.capacity > 0);
                    if write {
                        if full {
                            journal.records.pop_front();
                        }
                        journal.records.push_back(MeterRecord {
                            journal: index as u32 + 1,
                            phase_bits,
                            seen_s: instant_s,
                            cleared_s: f64::NAN,
                        });
                    }
                    journal.recorded = write;
                } else if journal.recorded {
                    if let Some(record) = journal.records.back_mut() {
                        record.phase_bits |= phase_bits;
                    }
                }
            } else if journal.present {
                journal.present = false;
                if journal.recorded {
                    if let Some(record) = journal.records.back_mut() {
                        record.cleared_s = instant_s;
                    }
                }
                journal.recorded = false;
            }
        }
    }

    /// The records of all journals: the low-voltage journal first, then the
    /// high-voltage one, then loss of voltage; oldest first within each.
    pub fn records(&self) -> impl Iterator<Item = &MeterRecord> {
        self.journals.iter().flat_map(|journal| journal.records.iter())
    }

    /// Number of records in `journal` (see [`journal`]).
    #[must_use]
    pub fn record_count(&self, journal: u32) -> usize {
        self.journals
            .get((journal as usize).wrapping_sub(1))
            .map_or(0, |journal| journal.records.len())
    }

    /// Journals that have displaced or refused a record since they were last
    /// cleared: bit `journal - 1`.
    #[must_use]
    pub fn overflow_bits(&self) -> u32 {
        self.journals
            .iter()
            .enumerate()
            .filter(|(_, journal)| journal.overflowed)
            .fold(0, |bits, (index, _)| bits | (1 << index))
    }

    /// Empty the journals and their overflow flags. A condition present now
    /// stays known to the meter, but its record is gone with the rest.
    pub fn clear_records(&mut self) {
        for journal in &mut self.journals {
            journal.records.clear();
            journal.recorded = false;
            journal.overflowed = false;
        }
    }

    /// Active energy exported [J].
    #[must_use]
    pub fn active_export_j(&self) -> f64 {
        self.active_export_j
    }

    /// Active energy imported [J].
    #[must_use]
    pub fn active_import_j(&self) -> f64 {
        self.active_import_j
    }

    /// Reactive energy exported [var s].
    #[must_use]
    pub fn reactive_export_var_s(&self) -> f64 {
        self.reactive_export_var_s
    }

    /// Reactive energy imported [var s].
    #[must_use]
    pub fn reactive_import_var_s(&self) -> f64 {
        self.reactive_import_var_s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f64 = 0.01;

    /// Feed `steps` sub-steps of `voltage` from sub-step number `*step`.
    fn run(meter: &mut RevenueMeter, step: &mut u32, steps: u32, voltage: [f64; 3]) {
        for _ in 0..steps {
            meter.observe(f64::from(*step) * DT, DT, voltage, 0.0, 0.0);
            *step += 1;
        }
    }

    #[test]
    fn a_dip_between_two_supervision_instants_leaves_no_record_and_one_across_an_instant_does() {
        // The same 130 ms dip from 10.40 s; only the place of the instants differs.
        let dip = |sample_phase_s: f64| {
            let mut meter = RevenueMeter::new(MeterCard::ASSUMED);
            meter.set_sample_phase_s(sample_phase_s);
            let mut step = 0;
            run(&mut meter, &mut step, 1_040, [1.0; 3]);
            run(&mut meter, &mut step, 13, [1.0, 0.69, 1.0]);
            run(&mut meter, &mut step, 300, [1.0; 3]);
            meter.records().copied().collect::<Vec<_>>()
        };
        // Instants at 10.0, 11.0: the dip (10.40 to 10.53) is between them.
        assert!(dip(0.0).is_empty());
        // Instants at 10.45, 11.45: one is inside.
        let records = dip(0.45);
        assert_eq!(records.len(), 1);
        let record = records[0];
        assert_eq!(record.journal, journal::LOW_VOLTAGE);
        assert_eq!(record.phase_bits, 0b010);
        // The record's times are the meter's instants, not the dip's start or end.
        assert_eq!(record.seen_s, 10.45);
        assert_eq!(record.cleared_s, 11.45);
        // Every sub-step of the dip, and no other, owns an instant for some phase.
        let hit = |centiseconds: u32| !dip(f64::from(centiseconds) * DT).is_empty();
        assert!(!hit(39) && hit(40) && hit(52) && !hit(53));
    }

    #[test]
    fn records_carry_the_phases_seen_and_the_instants_of_the_meters_own_clock() {
        let mut meter = RevenueMeter::new(MeterCard::ASSUMED);
        meter.set_clock_offset_s(1_000.0);
        meter.set_sample_phase_s(0.25);
        let mut step = 0;
        run(&mut meter, &mut step, 200, [1.0; 3]);
        // From 2.00 s: phase a low; from 3.50 s phase c joins; all back at 5.10 s.
        run(&mut meter, &mut step, 150, [0.8, 1.0, 1.0]);
        run(&mut meter, &mut step, 160, [0.8, 1.0, 0.7]);
        // An open record is in the journal already, without a closing instant.
        let open = meter.records().next().copied().expect("open record");
        assert!(open.cleared_s.is_nan());
        // Then phase b high and phase c lost at once, for 2 s.
        run(&mut meter, &mut step, 90, [1.0; 3]);
        run(&mut meter, &mut step, 200, [1.0, 1.2, 0.0]);
        run(&mut meter, &mut step, 200, [1.0; 3]);
        let records: Vec<MeterRecord> = meter.records().copied().collect();
        assert_eq!(records.len(), 3);
        let low = records[0];
        assert_eq!((low.journal, low.phase_bits), (journal::LOW_VOLTAGE, 0b101));
        // Model instants 2.25 and 5.25 (the disturbance ran from 2.00 to 5.10).
        assert_eq!((low.seen_s, low.cleared_s), (1_002.25, 1_005.25));
        let high = records[1];
        assert_eq!((high.journal, high.phase_bits), (journal::HIGH_VOLTAGE, 0b010));
        assert_eq!((high.seen_s, high.cleared_s), (1_006.25, 1_008.25));
        let lost = records[2];
        assert_eq!((lost.journal, lost.phase_bits), (journal::LOSS_OF_VOLTAGE, 0b100));
        assert_eq!(meter.record_count(journal::LOW_VOLTAGE), 1);
        assert_eq!(meter.overflow_bits(), 0);
        meter.clear_records();
        assert_eq!(meter.records().count(), 0);
    }

    #[test]
    fn a_full_journal_displaces_its_oldest_record_or_keeps_it_as_the_card_says() {
        for overflow in [JournalOverflow::OverwriteOldest, JournalOverflow::KeepOldest] {
            let card = MeterCard {
                loss_of_voltage_capacity: 3,
                overflow,
                ..MeterCard::ASSUMED
            };
            let mut meter = RevenueMeter::new(card);
            let mut step = 0;
            // Five losses of voltage, 2 s each, 2 s apart, from 1.5 s: seen at 2, 6, 10, 14, 18.
            run(&mut meter, &mut step, 150, [1.0; 3]);
            for event in 0..5 {
                run(&mut meter, &mut step, 200, [0.0, 1.0, 1.0]);
                run(&mut meter, &mut step, 200, [1.0; 3]);
                let expected = if event < 3 { 0 } else { 1 << (journal::LOSS_OF_VOLTAGE - 1) };
                assert_eq!(meter.overflow_bits(), expected, "after event {event}");
            }
            let seen: Vec<f64> = meter.records().map(|record| record.seen_s).collect();
            match overflow {
                JournalOverflow::OverwriteOldest => assert_eq!(seen, [10.0, 14.0, 18.0]),
                JournalOverflow::KeepOldest => assert_eq!(seen, [2.0, 6.0, 10.0]),
            }
            // Whatever is kept is complete: no closing instant was written to a wrong record.
            for record in meter.records() {
                assert_eq!(record.cleared_s, record.seen_s + 2.0);
            }
            assert_eq!(meter.record_count(journal::LOW_VOLTAGE), 0);
        }
    }

    #[test]
    fn energy_goes_to_the_register_of_its_sign() {
        let mut meter = RevenueMeter::new(MeterCard::ASSUMED);
        meter.observe(0.00, DT, [1.0; 3], 5_000.0, 2.0e5);
        meter.observe(0.01, DT, [1.0; 3], 5_000.0, -1.0e5);
        meter.observe(0.02, DT, [1.0; 3], -300.0, -1.0e5);
        assert_eq!(meter.active_export_j(), 10_000.0);
        assert_eq!(meter.active_import_j(), 300.0);
        assert_eq!(meter.reactive_export_var_s(), 2_000.0);
        assert_eq!(meter.reactive_import_var_s(), 2_000.0);
    }
}
