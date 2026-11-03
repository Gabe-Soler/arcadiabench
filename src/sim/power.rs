//! Hybrid site power: a fission baseline plus solar arrays feeding a battery
//! bank. Robots draw from the grid only while charging at a station.

use super::environment::Environment;
use super::params::Params;
use super::time::{SimMs, secs};

#[derive(Debug, Clone)]
pub struct Grid {
    pub fission_w: f64,
    pub solar_peak_w: f64,
    pub base_load_w: f64,
    pub bank_wh: f64,
    pub bank_capacity_wh: f64,
    low_frac: f64,
    low: bool,
    empty: bool,
}

/// A change in the bank's state worth telling the coordinator about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BankAlert {
    Low,
    Empty,
    Recovered,
}

impl Grid {
    pub fn new(p: &Params) -> Self {
        let bank = p.bank_capacity_wh * p.bank_initial_frac;
        Self {
            fission_w: p.fission_w,
            solar_peak_w: p.solar_peak_w,
            base_load_w: p.base_load_w,
            bank_wh: bank,
            bank_capacity_wh: p.bank_capacity_wh,
            low_frac: p.bank_low_frac,
            low: bank < p.bank_capacity_wh * p.bank_low_frac,
            empty: bank <= 0.0,
        }
    }

    pub fn solar_w(&self, env: &Environment, t: SimMs) -> f64 {
        self.solar_peak_w * Environment::solar_factor(t) * env.transmittance(t)
    }

    /// Account for `[t0, t1]` with `requested_w` of charging demand, using
    /// solar output at the midpoint. Returns the fraction of the requested
    /// charging power that could actually be delivered.
    pub fn settle(&mut self, env: &Environment, t0: SimMs, t1: SimMs, requested_w: f64) -> f64 {
        if t1 <= t0 {
            return 1.0;
        }
        let dt_h = secs(t1 - t0) / 3600.0;
        let supply = self.fission_w + self.solar_w(env, t0 + (t1 - t0) / 2);
        let surplus_w = supply - self.base_load_w - requested_w;
        if surplus_w >= 0.0 {
            self.bank_wh = (self.bank_wh + surplus_w * dt_h).min(self.bank_capacity_wh);
            return 1.0;
        }
        let deficit_wh = -surplus_w * dt_h;
        if self.bank_wh >= deficit_wh {
            self.bank_wh -= deficit_wh;
            return 1.0;
        }
        // The bank runs dry: charging gets whatever supply and the remaining bank allow.
        let available_w = (supply - self.base_load_w).max(0.0) + self.bank_wh / dt_h;
        self.bank_wh = 0.0;
        if requested_w <= 0.0 {
            1.0
        } else {
            (available_w / requested_w).clamp(0.0, 1.0)
        }
    }

    /// Detects threshold crossings since the last call.
    pub fn alert(&mut self) -> Option<BankAlert> {
        let low = self.bank_wh < self.bank_capacity_wh * self.low_frac;
        let empty = self.bank_wh <= 1e-6;
        let alert = if empty && !self.empty {
            Some(BankAlert::Empty)
        } else if low && !self.low {
            Some(BankAlert::Low)
        } else if !low && self.low {
            Some(BankAlert::Recovered)
        } else {
            None
        };
        self.low = low;
        self.empty = empty;
        alert
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::time::SOL_MS;

    fn calm_env() -> Environment {
        Environment::generate(
            0,
            SOL_MS,
            &Params {
                storm_chance_per_sol: 0.0,
                ..Params::default()
            },
        )
    }

    #[test]
    fn energy_balance_over_a_sol() {
        let p = Params::default();
        let env = calm_env();
        let mut g = Grid::new(&Params {
            bank_capacity_wh: 1e9,
            ..p.clone()
        });
        let start = g.bank_wh;
        let step = 60_000;
        let mut expected = 0.0;
        let mut t = 0;
        while t < SOL_MS {
            let f = g.settle(&env, t, t + step, 1000.0);
            assert_eq!(f, 1.0);
            let mid = t + step / 2;
            expected +=
                (p.fission_w + g.solar_w(&env, mid) - p.base_load_w - 1000.0) * 60.0 / 3600.0;
            t += step;
        }
        assert!((g.bank_wh - start - expected).abs() < 1e-3);
        // Fission alone exceeds the base load, so a sol nets positive energy.
        assert!(g.bank_wh > start);
    }

    #[test]
    fn charging_is_throttled_when_the_bank_is_empty_at_night() {
        let p = Params {
            bank_initial_frac: 0.0,
            ..Params::default()
        };
        let mut g = Grid::new(&p);
        // Night: supply = fission only (10 kW) minus base 2 kW leaves 8 kW for 16 kW of demand.
        let f = g.settle(&calm_env(), 0, 600_000, 16_000.0);
        assert!((f - 0.5).abs() < 1e-9, "{f}");
        assert_eq!(g.bank_wh, 0.0);
    }

    #[test]
    fn alerts_fire_once_per_crossing() {
        let mut g = Grid::new(&Params::default());
        assert_eq!(g.alert(), None);
        g.bank_wh = 10_000.0;
        assert_eq!(g.alert(), Some(BankAlert::Low));
        assert_eq!(g.alert(), None);
        g.bank_wh = 0.0;
        assert_eq!(g.alert(), Some(BankAlert::Empty));
        g.bank_wh = 50_000.0;
        assert_eq!(g.alert(), Some(BankAlert::Recovered));
    }
}
