//! Spend meter (spec §7.4): daily and monthly caps enforced in the backend from the
//! token usage the provider reports. Amounts are integer micro-dollars (1e-6 USD) so
//! there is no float rounding. Time is injected, so the meter is deterministic to test.

use super::provider::Usage;
use std::sync::{Mutex, MutexGuard};

pub type Micros = u64;

/// Price per one million tokens, in micro-dollars. Comes from settings, never hard-coded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Price {
    pub input_per_mtok: Micros,
    pub output_per_mtok: Micros,
}

impl Price {
    pub fn cost(&self, usage: Usage) -> Micros {
        let total = usage.input_tokens as u128 * self.input_per_mtok as u128
            + usage.output_tokens as u128 * self.output_per_mtok as u128;
        // Round up so a tiny turn is never free.
        total.div_ceil(1_000_000) as Micros
    }
}

/// `None` means no cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Caps {
    pub daily: Option<Micros>,
    pub monthly: Option<Micros>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapKind {
    Daily,
    Monthly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapReached {
    pub kind: CapKind,
    pub cap: Micros,
    pub spent: Micros,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Day {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Day {
    /// Civil date (UTC) from Unix seconds. Howard Hinnant's days-to-civil algorithm.
    pub fn from_unix_secs(secs: u64) -> Self {
        let z = (secs / 86_400) as i64 + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
        Self { year, month, day }
    }
}

/// What the settings page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpendTotals {
    pub today: Micros,
    pub this_month: Micros,
}

struct State {
    day: Day,
    totals: SpendTotals,
}

pub struct SpendMeter {
    caps: Mutex<Caps>,
    state: Mutex<State>,
}

impl SpendMeter {
    pub fn new(caps: Caps, now: Day) -> Self {
        Self::restore(caps, now, SpendTotals::default())
    }

    /// For loading saved totals at start-up (storage arrives with M5).
    pub fn restore(caps: Caps, now: Day, totals: SpendTotals) -> Self {
        Self {
            caps: Mutex::new(caps),
            state: Mutex::new(State { day: now, totals }),
        }
    }

    fn state(&self, now: Day) -> MutexGuard<'_, State> {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if now != s.day {
            if (now.year, now.month) != (s.day.year, s.day.month) {
                s.totals.this_month = 0;
            }
            s.totals.today = 0;
            s.day = now;
        }
        s
    }

    pub fn set_caps(&self, caps: Caps) {
        *self.caps.lock().unwrap_or_else(|e| e.into_inner()) = caps;
    }

    pub fn totals(&self, now: Day) -> SpendTotals {
        self.state(now).totals
    }

    /// Call before every cloud request. Blocks once spending has reached a cap.
    pub fn check(&self, now: Day) -> Result<(), CapReached> {
        let caps = *self.caps.lock().unwrap_or_else(|e| e.into_inner());
        let t = self.state(now).totals;
        if let Some(cap) = caps.daily
            && t.today >= cap
        {
            return Err(CapReached {
                kind: CapKind::Daily,
                cap,
                spent: t.today,
            });
        }
        if let Some(cap) = caps.monthly
            && t.this_month >= cap
        {
            return Err(CapReached {
                kind: CapKind::Monthly,
                cap,
                spent: t.this_month,
            });
        }
        Ok(())
    }

    /// Records a finished (or cancelled) request from the provider's reported usage.
    pub fn record(&self, now: Day, usage: Usage, price: Price) -> Micros {
        let cost = price.cost(usage);
        let mut s = self.state(now);
        s.totals.today = s.totals.today.saturating_add(cost);
        s.totals.this_month = s.totals.this_month.saturating_add(cost);
        cost
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D1: Day = Day {
        year: 2026,
        month: 10,
        day: 31,
    };
    const D2: Day = Day {
        year: 2026,
        month: 11,
        day: 1,
    };
    const D3: Day = Day {
        year: 2026,
        month: 11,
        day: 2,
    };
    // $1 per million input, $5 per million output.
    const PRICE: Price = Price {
        input_per_mtok: 1_000_000,
        output_per_mtok: 5_000_000,
    };

    fn use_(i: u32, o: u32) -> Usage {
        Usage {
            input_tokens: i,
            output_tokens: o,
        }
    }

    #[test]
    fn cost_is_exact_and_rounds_up() {
        assert_eq!(PRICE.cost(use_(1_000_000, 0)), 1_000_000);
        assert_eq!(PRICE.cost(use_(2_000, 500)), 2_000 + 2_500);
        assert_eq!(PRICE.cost(use_(1, 0)), 1, "a tiny turn is never free");
        assert_eq!(PRICE.cost(use_(0, 0)), 0);
    }

    #[test]
    fn no_caps_never_blocks() {
        let m = SpendMeter::new(Caps::default(), D1);
        m.record(D1, use_(10_000_000, 10_000_000), PRICE);
        assert!(m.check(D1).is_ok());
    }

    #[test]
    fn daily_cap_blocks_when_reached_not_before() {
        let m = SpendMeter::new(
            Caps {
                daily: Some(5_000),
                monthly: None,
            },
            D1,
        );
        m.record(D1, use_(4_999, 0), PRICE);
        assert!(m.check(D1).is_ok());
        m.record(D1, use_(1, 0), PRICE);
        assert_eq!(
            m.check(D1),
            Err(CapReached {
                kind: CapKind::Daily,
                cap: 5_000,
                spent: 5_000
            })
        );
    }

    #[test]
    fn daily_resets_next_day_but_monthly_keeps_counting() {
        let m = SpendMeter::new(
            Caps {
                daily: Some(5_000),
                monthly: Some(8_000),
            },
            D2,
        );
        m.record(D2, use_(5_000, 0), PRICE);
        assert_eq!(m.check(D2).unwrap_err().kind, CapKind::Daily);
        m.record(D3, use_(2_000, 0), PRICE); // new day: daily reset, month total 7_000
        assert!(m.check(D3).is_ok());
        assert_eq!(
            m.totals(D3),
            SpendTotals {
                today: 2_000,
                this_month: 7_000
            }
        );
        m.record(D3, use_(1_000, 0), PRICE);
        assert_eq!(m.check(D3).unwrap_err().kind, CapKind::Monthly);
    }

    #[test]
    fn month_rollover_resets_both() {
        let m = SpendMeter::new(
            Caps {
                daily: None,
                monthly: Some(1_000),
            },
            D1,
        );
        m.record(D1, use_(1_000, 0), PRICE);
        assert!(m.check(D1).is_err());
        assert!(m.check(D2).is_ok());
        assert_eq!(m.totals(D2), SpendTotals::default());
    }

    #[test]
    fn raising_the_cap_unblocks() {
        let m = SpendMeter::new(
            Caps {
                daily: Some(100),
                monthly: None,
            },
            D1,
        );
        m.record(D1, use_(100, 0), PRICE);
        assert!(m.check(D1).is_err());
        m.set_caps(Caps {
            daily: Some(1_000),
            monthly: None,
        });
        assert!(m.check(D1).is_ok());
    }

    #[test]
    fn restore_resumes_saved_totals() {
        let m = SpendMeter::restore(
            Caps {
                daily: Some(500),
                monthly: None,
            },
            D1,
            SpendTotals {
                today: 500,
                this_month: 900,
            },
        );
        assert!(m.check(D1).is_err());
    }

    #[test]
    fn civil_date_from_unix_seconds() {
        assert_eq!(
            Day::from_unix_secs(0),
            Day {
                year: 1970,
                month: 1,
                day: 1
            }
        );
        assert_eq!(
            Day::from_unix_secs(1_700_000_000),
            Day {
                year: 2023,
                month: 11,
                day: 14
            }
        );
        assert_eq!(
            Day::from_unix_secs(951_782_400),
            Day {
                year: 2000,
                month: 2,
                day: 29
            }
        );
        assert_eq!(
            Day::from_unix_secs(1_793_491_200),
            Day {
                year: 2026,
                month: 11,
                day: 1
            }
        );
    }
}
