//! The landing-pad goal: what "done" means, progress, and verification.
//!
//! The bench does not plan; it only judges the ground truth against the goal.
//! A pad cell counts once it is in the required surface state, boulder-free and
//! within tolerance. The goal is complete when every pad cell counts, slopes are
//! within limits, the berm is built, and every pad cell has been scanned since
//! it last changed (the outline's "verify" step).

use super::terrain::{Terrain, rect_cells};
use super::time::SimMs;
use arcadia_protocol::{GridCoord, GridRect, LandingPadSpec, Surface};

#[derive(Debug, Clone)]
pub struct Mission {
    pub goal_id: String,
    pub pad: LandingPadSpec,
    /// Berm cells with their elevation at mission start.
    pub berm: Vec<(GridCoord, f64)>,
    verified: Vec<bool>,
    last_change: Vec<SimMs>,
}

impl Mission {
    pub fn new(goal_id: &str, pad: LandingPadSpec, terrain: &Terrain) -> Self {
        let n = super::terrain::rect_cell_count(&pad.area) as usize;
        let berm = pad
            .berm
            .iter()
            .flat_map(|b| b.cells.iter())
            .map(|&c| (c, terrain.get(c).elevation_m))
            .collect();
        Self {
            goal_id: goal_id.into(),
            pad,
            berm,
            verified: vec![false; n],
            last_change: vec![0; n],
        }
    }

    fn local(&self, c: GridCoord) -> Option<usize> {
        let a = &self.pad.area;
        if c.x < a.min.x || c.x > a.max.x || c.y < a.min.y || c.y > a.max.y {
            return None;
        }
        let w = a.max.x - a.min.x + 1;
        Some(((c.y - a.min.y) * w + (c.x - a.min.x)) as usize)
    }

    pub fn in_pad(&self, c: GridCoord) -> bool {
        self.local(c).is_some()
    }

    pub fn is_berm(&self, c: GridCoord) -> bool {
        self.berm.iter().any(|&(b, _)| b == c)
    }

    pub fn on_change(&mut self, c: GridCoord, now: SimMs) {
        if let Some(i) = self.local(c) {
            self.verified[i] = false;
            self.last_change[i] = now;
        }
    }

    /// A scan that started at `scan_start` verifies every pad cell in `area`
    /// that has not changed since the scan began.
    pub fn on_scan(&mut self, area: &GridRect, scan_start: SimMs) {
        for c in rect_cells(area) {
            if let Some(i) = self.local(c)
                && self.last_change[i] <= scan_start
            {
                self.verified[i] = true;
            }
        }
    }

    fn required_surface(&self) -> Surface {
        if self.pad.stabilize {
            Surface::Stabilized
        } else {
            Surface::Compacted
        }
    }

    pub fn cell_done(&self, terrain: &Terrain, c: GridCoord) -> bool {
        let t = terrain.get(c);
        t.surface == self.required_surface()
            && t.boulders.is_empty()
            && (t.elevation_m - self.pad.target_elevation_m).abs() <= self.pad.surface_tolerance_m
    }

    /// Fraction of pad cells that meet the spec.
    pub fn progress(&self, terrain: &Terrain) -> f64 {
        let cells: Vec<_> = rect_cells(&self.pad.area).collect();
        let done = cells
            .iter()
            .filter(|&&c| self.cell_done(terrain, c))
            .count();
        done as f64 / cells.len() as f64
    }

    fn slopes_ok(&self, terrain: &Terrain) -> bool {
        let max_rise = libm::tan(self.pad.max_slope_deg.to_radians()) * terrain.cell_size_m;
        rect_cells(&self.pad.area).all(|c| {
            let here = terrain.get(c).elevation_m;
            [
                GridCoord { x: c.x + 1, y: c.y },
                GridCoord { x: c.x, y: c.y + 1 },
            ]
            .into_iter()
            .filter(|&n| self.in_pad(n))
            .all(|n| (terrain.get(n).elevation_m - here).abs() <= max_rise)
        })
    }

    fn berm_ok(&self, terrain: &Terrain) -> bool {
        let Some(spec) = &self.pad.berm else {
            return true;
        };
        self.berm.iter().all(|&(c, base)| {
            let t = terrain.get(c);
            t.surface == Surface::Berm && t.elevation_m >= base + spec.height_m
        })
    }

    pub fn complete(&self, terrain: &Terrain) -> bool {
        self.verified.iter().all(|&v| v)
            && rect_cells(&self.pad.area).all(|c| self.cell_done(terrain, c))
            && self.slopes_ok(terrain)
            && self.berm_ok(terrain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::params::Params;
    use crate::sim::rng::Rng;
    use arcadia_protocol::BermSpec;

    fn setup() -> (Terrain, Mission) {
        let terrain = Terrain::generate(&mut Rng::new(1), 10, 10, 1.0, &Params::default());
        let pad = LandingPadSpec {
            area: GridRect {
                min: GridCoord { x: 2, y: 2 },
                max: GridCoord { x: 4, y: 4 },
            },
            target_elevation_m: -4102.0,
            surface_tolerance_m: 0.05,
            max_slope_deg: 2.0,
            stabilize: true,
            berm: Some(BermSpec {
                cells: vec![GridCoord { x: 7, y: 7 }],
                height_m: 0.5,
            }),
        };
        let m = Mission::new("pad", pad, &terrain);
        (terrain, m)
    }

    fn finish_pad(terrain: &mut Terrain, m: &Mission) {
        for c in rect_cells(&m.pad.area) {
            let cell = terrain.get_mut(c);
            cell.surface = Surface::Stabilized;
            cell.boulders.clear();
            cell.elevation_m = -4102.0;
        }
        let (b, base) = m.berm[0];
        terrain.get_mut(b).surface = Surface::Berm;
        terrain.get_mut(b).elevation_m = base + 0.6;
    }

    #[test]
    fn complete_needs_a_scan_after_the_last_change() {
        let (mut terrain, mut m) = setup();
        finish_pad(&mut terrain, &m);
        assert_eq!(m.progress(&terrain), 1.0);
        assert!(!m.complete(&terrain), "not verified yet");

        m.on_change(GridCoord { x: 3, y: 3 }, 100);
        m.on_scan(&m.pad.area.clone(), 50); // scan started before the change
        assert!(!m.complete(&terrain));
        m.on_scan(&m.pad.area.clone(), 100);
        assert!(m.complete(&terrain));
    }

    #[test]
    fn steep_step_or_missing_berm_blocks_completion() {
        let (mut terrain, mut m) = setup();
        finish_pad(&mut terrain, &m);
        m.on_scan(&m.pad.area.clone(), 0);
        terrain.get_mut(GridCoord { x: 3, y: 3 }).elevation_m = -4101.96; // in tolerance, 2.3° step
        assert!(!m.complete(&terrain));
        terrain.get_mut(GridCoord { x: 3, y: 3 }).elevation_m = -4102.0;
        assert!(m.complete(&terrain));
        terrain.get_mut(GridCoord { x: 7, y: 7 }).surface = Surface::Stockpile;
        assert!(!m.complete(&terrain));
    }
}
