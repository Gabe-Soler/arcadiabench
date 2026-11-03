//! Ground-truth terrain grid and the physical operations robots perform on it.

use super::params::Params;
use super::rng::Rng;
use arcadia_protocol::{
    Boulder, Cell, CellObservation, GridCoord, GridRect, Regolith, SiteMap, Surface,
};
use std::f64::consts::PI;

#[derive(Debug, Clone, PartialEq)]
pub struct TCell {
    pub elevation_m: f64,
    pub regolith_depth_m: f64,
    pub density_kg_m3: f64,
    pub cohesion_kpa: f64,
    /// Boulder diameters, m.
    pub boulders: Vec<f64>,
    pub surface: Surface,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Terrain {
    pub width: u32,
    pub height: u32,
    pub cell_size_m: f64,
    pub cells: Vec<TCell>,
}

/// Smooth value noise on a lattice with `spacing` cells between knots.
struct ValueNoise {
    spacing: f64,
    cols: usize,
    knots: Vec<f64>,
}

impl ValueNoise {
    fn new(rng: &mut Rng, width: u32, height: u32, spacing: f64) -> Self {
        let cols = (f64::from(width) / spacing).ceil() as usize + 2;
        let rows = (f64::from(height) / spacing).ceil() as usize + 2;
        let knots = (0..cols * rows).map(|_| rng.range(-1.0, 1.0)).collect();
        Self {
            spacing,
            cols,
            knots,
        }
    }

    fn at(&self, x: f64, y: f64) -> f64 {
        let (gx, gy) = (x / self.spacing, y / self.spacing);
        let (ix, iy) = (gx.floor() as usize, gy.floor() as usize);
        let smooth = |t: f64| t * t * (3.0 - 2.0 * t);
        let (tx, ty) = (smooth(gx - gx.floor()), smooth(gy - gy.floor()));
        let k = |i: usize, j: usize| self.knots[j * self.cols + i];
        let top = k(ix, iy) * (1.0 - tx) + k(ix + 1, iy) * tx;
        let bottom = k(ix, iy + 1) * (1.0 - tx) + k(ix + 1, iy + 1) * tx;
        top * (1.0 - ty) + bottom * ty
    }
}

impl Terrain {
    /// A gently tilted plain with multi-octave relief, variable regolith and a
    /// sparse power-law boulder field.
    pub fn generate(rng: &mut Rng, width: u32, height: u32, cell_size_m: f64, p: &Params) -> Self {
        let tilt = (rng.normal() * p.tilt_sigma, rng.normal() * p.tilt_sigma);
        let relief: Vec<(ValueNoise, f64)> = p
            .noise_octaves
            .iter()
            .map(|&(spacing, amp)| (ValueNoise::new(rng, width, height, spacing), amp))
            .collect();
        let depth = ValueNoise::new(rng, width, height, 12.0);
        let density = ValueNoise::new(rng, width, height, 20.0);
        let cohesion = ValueNoise::new(rng, width, height, 20.0);
        let lerp = |(lo, hi): (f64, f64), n: f64| lo + (hi - lo) * (n + 1.0) / 2.0;

        let mut cells = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                let (fx, fy) = (f64::from(x), f64::from(y));
                let elevation_m = p.base_elevation_m
                    + (tilt.0 * fx + tilt.1 * fy) * cell_size_m
                    + relief
                        .iter()
                        .map(|(n, amp)| n.at(fx, fy) * amp)
                        .sum::<f64>();
                let boulders = (0..rng.poisson(p.boulders_per_cell))
                    .map(|_| {
                        let u = 1.0 - rng.uniform();
                        (p.boulder_d_min_m * libm::pow(u, -1.0 / p.boulder_alpha))
                            .min(p.boulder_d_max_m)
                    })
                    .collect();
                cells.push(TCell {
                    elevation_m,
                    regolith_depth_m: lerp(p.regolith_depth_m, depth.at(fx, fy)),
                    density_kg_m3: lerp(p.regolith_density_kg_m3, density.at(fx, fy)),
                    cohesion_kpa: lerp(p.regolith_cohesion_kpa, cohesion.at(fx, fy)),
                    boulders,
                    surface: Surface::Natural,
                });
            }
        }
        Self {
            width,
            height,
            cell_size_m,
            cells,
        }
    }

    pub fn in_bounds(&self, c: GridCoord) -> bool {
        c.x < self.width && c.y < self.height
    }

    pub fn rect_in_bounds(&self, r: &GridRect) -> bool {
        r.min.x <= r.max.x && r.min.y <= r.max.y && self.in_bounds(r.max)
    }

    fn idx(&self, c: GridCoord) -> usize {
        (c.y * self.width + c.x) as usize
    }

    pub fn get(&self, c: GridCoord) -> &TCell {
        &self.cells[self.idx(c)]
    }

    pub fn get_mut(&mut self, c: GridCoord) -> &mut TCell {
        let i = self.idx(c);
        &mut self.cells[i]
    }

    pub fn cell_area_m2(&self) -> f64 {
        self.cell_size_m * self.cell_size_m
    }

    pub fn boulder_volume_m3(&self, c: GridCoord) -> f64 {
        self.get(c)
            .boulders
            .iter()
            .map(|d| PI / 6.0 * d * d * d)
            .sum()
    }

    /// Cut the cell down to `target`; returns the volume removed (m³).
    pub fn cut_to(&mut self, c: GridCoord, target: f64) -> f64 {
        let area = self.cell_area_m2();
        let cell = self.get_mut(c);
        let depth = (cell.elevation_m - target).max(0.0);
        cell.elevation_m -= depth;
        cell.regolith_depth_m = (cell.regolith_depth_m - depth).max(0.0);
        depth * area
    }

    /// Loose material that can be loaded from the cell, kg.
    pub fn available_mass_kg(&self, c: GridCoord) -> f64 {
        let cell = self.get(c);
        cell.regolith_depth_m * self.cell_area_m2() * cell.density_kg_m3
    }

    /// Remove up to `kg` of loose material; returns the mass actually removed.
    pub fn remove_mass(&mut self, c: GridCoord, kg: f64) -> f64 {
        let take = kg.min(self.available_mass_kg(c)).max(0.0);
        let area = self.cell_area_m2();
        let cell = self.get_mut(c);
        let dh = take / (cell.density_kg_m3 * area);
        cell.elevation_m -= dh;
        cell.regolith_depth_m = (cell.regolith_depth_m - dh).max(0.0);
        take
    }

    /// Deposit `kg` of material on the cell.
    pub fn add_mass(&mut self, c: GridCoord, kg: f64) {
        let area = self.cell_area_m2();
        let cell = self.get_mut(c);
        let dh = kg / (cell.density_kg_m3 * area);
        cell.elevation_m += dh;
        cell.regolith_depth_m += dh;
    }

    /// Full ground truth for one cell, in protocol form.
    pub fn truth_cell(&self, c: GridCoord) -> Cell {
        let t = self.get(c);
        Cell {
            elevation_m: Some(t.elevation_m),
            regolith: Some(Regolith {
                depth_m: Some(t.regolith_depth_m),
                bulk_density_kg_m3: Some(t.density_kg_m3),
                cohesion_kpa: Some(t.cohesion_kpa),
            }),
            boulders: Some(
                t.boulders
                    .iter()
                    .map(|&d| Boulder {
                        diameter_m: d,
                        mass_kg: None,
                    })
                    .collect(),
            ),
            surface: Some(t.surface),
            ice_depth_m: None,
        }
    }

    pub fn truth_observation(&self, c: GridCoord) -> CellObservation {
        CellObservation {
            at: c,
            cell: self.truth_cell(c),
        }
    }

    pub fn to_site_map(&self, site_id: &str, name: &str) -> SiteMap {
        SiteMap {
            site_id: site_id.into(),
            name: Some(name.into()),
            width: self.width,
            height: self.height,
            cell_size_m: self.cell_size_m,
            cells: self.coords().map(|c| self.truth_observation(c)).collect(),
        }
    }

    pub fn coords(&self) -> impl Iterator<Item = GridCoord> + '_ {
        (0..self.height).flat_map(move |y| (0..self.width).map(move |x| GridCoord { x, y }))
    }
}

pub fn rect_cells(r: &GridRect) -> impl Iterator<Item = GridCoord> + '_ {
    (r.min.y..=r.max.y).flat_map(move |y| (r.min.x..=r.max.x).map(move |x| GridCoord { x, y }))
}

/// Row-by-row, alternating direction, so a robot never doubles back across a row.
pub fn serpentine(r: &GridRect) -> Vec<GridCoord> {
    let mut out = Vec::new();
    for (i, y) in (r.min.y..=r.max.y).enumerate() {
        let xs: Vec<u32> = (r.min.x..=r.max.x).collect();
        let row: Box<dyn Iterator<Item = &u32>> = if i % 2 == 0 {
            Box::new(xs.iter())
        } else {
            Box::new(xs.iter().rev())
        };
        out.extend(row.map(|&x| GridCoord { x, y }));
    }
    out
}

pub fn rect_cell_count(r: &GridRect) -> u64 {
    u64::from(r.max.x - r.min.x + 1) * u64::from(r.max.y - r.min.y + 1)
}

pub fn distance_cells(a: GridCoord, b: GridCoord) -> f64 {
    let dx = f64::from(a.x) - f64::from(b.x);
    let dy = f64::from(a.y) - f64::from(b.y);
    (dx * dx + dy * dy).sqrt()
}

/// The cell of `r` nearest to `c`.
pub fn clamp_into(c: GridCoord, r: &GridRect) -> GridCoord {
    GridCoord {
        x: c.x.clamp(r.min.x, r.max.x),
        y: c.y.clamp(r.min.y, r.max.y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terrain(seed: u64) -> Terrain {
        Terrain::generate(
            &mut Rng::stream(seed, "terrain"),
            60,
            60,
            1.0,
            &Params::default(),
        )
    }

    #[test]
    fn generation_is_reproducible_and_plausible() {
        let a = terrain(3);
        assert_eq!(a, terrain(3));
        assert_ne!(a, terrain(4));
        let p = Params::default();
        for c in &a.cells {
            assert!(
                (c.elevation_m - p.base_elevation_m).abs() < 2.0,
                "{}",
                c.elevation_m
            );
            assert!((p.regolith_depth_m.0..=p.regolith_depth_m.1).contains(&c.regolith_depth_m));
            assert!(
                c.boulders
                    .iter()
                    .all(|&d| d >= p.boulder_d_min_m && d <= p.boulder_d_max_m)
            );
        }
        let boulders: usize = a.cells.iter().map(|c| c.boulders.len()).sum();
        assert!(boulders > 50 && boulders < 500, "{boulders}");
    }

    #[test]
    fn mass_is_conserved_between_cells() {
        let mut t = terrain(1);
        let (a, b) = (GridCoord { x: 1, y: 1 }, GridCoord { x: 2, y: 1 });
        let mass = |t: &Terrain, c| t.get(c).elevation_m * t.get(c).density_kg_m3;
        let before = mass(&t, a) + mass(&t, b);
        let moved = t.remove_mass(a, 300.0);
        t.add_mass(b, moved);
        assert!((moved - 300.0).abs() < 1e-9);
        assert!((mass(&t, a) + mass(&t, b) - before).abs() < 1e-6);
    }

    #[test]
    fn serpentine_alternates() {
        let r = GridRect {
            min: GridCoord { x: 0, y: 0 },
            max: GridCoord { x: 2, y: 1 },
        };
        let xs: Vec<(u32, u32)> = serpentine(&r).iter().map(|c| (c.x, c.y)).collect();
        assert_eq!(xs, vec![(0, 0), (1, 0), (2, 0), (2, 1), (1, 1), (0, 1)]);
    }
}
