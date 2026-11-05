const W: i32 = 18;
const LAYER: i32 = W * W;
const LAVA_BIT: u8 = 0x80;
/// How far around the window the quart grid reaches, in blocks: vanilla's
/// largest Biome Blend radius (7), rounded up to whole quarts.
pub const BLEND_REACH: i32 = 8;
/// The quart grid's width: 16 blocks and the reach either side.
const QUARTS: i32 = (16 + 2 * BLEND_REACH) / 4;

pub struct Occupancy {
    x_base: i32,
    z_base: i32,
    y_base: i32,
    y_count: i32,
    solid: Vec<u64>,
    full: Vec<u64>,
    light: Vec<u8>,
    emission: Vec<u8>,
    emissive: Vec<u64>,
    fluid: Vec<u8>,
    cull_group: Vec<u32>,
    biome: Vec<u8>,
    /// Biomes by quart (4 blocks a side, as chunks store them) over the
    /// window and 8 blocks around it, for biome blending (`blend`).
    quarts: Vec<u8>,
    quart_x0: i32,
    quart_z0: i32,
    quart_y0: i32,
    quart_ny: i32,
}

impl Occupancy {
    pub fn new(x_base: i32, z_base: i32, y_base: i32, y_count: i32) -> Self {
        let mut occ = Self {
            x_base: 0,
            z_base: 0,
            y_base: 0,
            y_count: 0,
            solid: Vec::new(),
            full: Vec::new(),
            light: Vec::new(),
            emission: Vec::new(),
            emissive: Vec::new(),
            fluid: Vec::new(),
            cull_group: Vec::new(),
            biome: Vec::new(),
            quarts: Vec::new(),
            quart_x0: 0,
            quart_z0: 0,
            quart_y0: 0,
            quart_ny: 0,
        };
        occ.reset(x_base, z_base, y_base, y_count);
        occ
    }

    pub fn reset(&mut self, x_base: i32, z_base: i32, y_base: i32, y_count: i32) {
        self.x_base = x_base;
        self.z_base = z_base;
        self.y_base = y_base;
        self.y_count = y_count;
        let cells = (LAYER * y_count) as usize;
        self.solid.clear();
        self.solid.resize(cells.div_ceil(64), 0);
        self.full.clear();
        self.full.resize(cells.div_ceil(64), 0);
        self.fluid.clear();
        self.fluid.resize(cells, 0);
        self.emission.clear();
        self.emission.resize(cells, 0);
        self.emissive.clear();
        self.emissive.resize(cells.div_ceil(64), 0);
        self.light.clear();
        self.light.resize(cells, 0xF0);
        self.cull_group.clear();
        self.cull_group.resize(cells, 0);
        self.biome.clear();
        self.biome.resize(cells, u8::MAX);
        self.quart_x0 = (x_base - BLEND_REACH).div_euclid(4);
        self.quart_z0 = (z_base - BLEND_REACH).div_euclid(4);
        self.quart_y0 = y_base.div_euclid(4);
        self.quart_ny = (y_base + y_count - 1).div_euclid(4) - self.quart_y0 + 1;
        self.quarts.clear();
        self.quarts
            .resize((QUARTS * QUARTS * self.quart_ny) as usize, u8::MAX);
    }

    fn quart_index(&self, qx: i32, qy: i32, qz: i32) -> Option<usize> {
        let (x, y, z) = (qx - self.quart_x0, qy - self.quart_y0, qz - self.quart_z0);
        if x < 0 || x >= QUARTS || z < 0 || z >= QUARTS || y < 0 || y >= self.quart_ny {
            return None;
        }
        Some(((y * QUARTS + x) * QUARTS + z) as usize)
    }

    /// Sets the biome of the quart at quart coordinates (block / 4).
    pub fn set_quart(&mut self, qx: i32, qy: i32, qz: i32, biome: u8) {
        if let Some(i) = self.quart_index(qx, qy, qz) {
            self.quarts[i] = biome;
        }
    }

    /// Whether the quart grid reaches quart column `(qx, qz)`.
    pub fn quart_column_in_range(&self, qx: i32, qz: i32) -> bool {
        (0..QUARTS).contains(&(qx - self.quart_x0)) && (0..QUARTS).contains(&(qz - self.quart_z0))
    }

    /// The quart range the grid covers in y.
    pub fn quart_y_range(&self) -> (i32, i32) {
        (self.quart_y0, self.quart_y0 + self.quart_ny)
    }

    /// A colour from `f` for the biome at a block, averaged over the
    /// (2 × `radius` + 1)² blocks around it at its height, as vanilla's
    /// `BlockTintCache` blends (the Biome Blend option). Blocks whose biome
    /// isn't known count for nothing.
    pub fn blend(
        &self,
        x: i32,
        y: i32,
        z: i32,
        radius: i32,
        f: impl Fn(u8) -> [f32; 3],
    ) -> [f32; 3] {
        let own = || f(self.biome(x, y, z));
        if radius <= 0 {
            return own();
        }
        let radius = radius.min(BLEND_REACH);
        let qy = y.div_euclid(4);
        let (mut sum, mut n) = ([0.0f32; 3], 0u32);
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let Some(i) = self.quart_index((x + dx).div_euclid(4), qy, (z + dz).div_euclid(4))
                else {
                    continue;
                };
                let b = self.quarts[i];
                if b == u8::MAX {
                    continue;
                }
                let c = f(b);
                for k in 0..3 {
                    sum[k] += c[k];
                }
                n += 1;
            }
        }
        if n == 0 {
            return own();
        }
        sum.map(|s| s / n as f32)
    }

    #[inline]
    pub fn covers_y(&self, y: i32) -> bool {
        y >= self.y_base && y < self.y_base + self.y_count
    }

    #[inline]
    fn index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        let px = x - self.x_base + 1;
        let pz = z - self.z_base + 1;
        let py = y - self.y_base;
        if px < 0 || px >= W || pz < 0 || pz >= W || py < 0 || py >= self.y_count {
            return None;
        }
        Some((py * LAYER + px * W + pz) as usize)
    }

    #[inline]
    pub fn set_solid(&mut self, x: i32, y: i32, z: i32) {
        if let Some(i) = self.index(x, y, z) {
            bit_set(&mut self.solid, i);
        }
    }

    #[inline]
    pub fn set_full_cube(&mut self, x: i32, y: i32, z: i32) {
        if let Some(i) = self.index(x, y, z) {
            bit_set(&mut self.full, i);
        }
    }

    #[inline]
    pub fn set_emission(&mut self, x: i32, y: i32, z: i32, emission: u8) {
        if let Some(i) = self.index(x, y, z) {
            self.emission[i] = emission;
        }
    }

    #[inline]
    pub fn set_emissive(&mut self, x: i32, y: i32, z: i32) {
        if let Some(i) = self.index(x, y, z) {
            bit_set(&mut self.emissive, i);
        }
    }

    #[inline]
    pub fn is_emissive(&self, x: i32, y: i32, z: i32) -> bool {
        match self.index(x, y, z) {
            Some(i) => bit_get(&self.emissive, i),
            None => false,
        }
    }

    #[inline]
    pub fn set_cull_group(&mut self, x: i32, y: i32, z: i32, group: u32) {
        if let Some(i) = self.index(x, y, z) {
            self.cull_group[i] = group;
        }
    }

    #[inline]
    pub fn set_biome(&mut self, x: i32, y: i32, z: i32, biome: u8) {
        if let Some(i) = self.index(x, y, z) {
            self.biome[i] = biome;
        }
    }

    #[inline]
    pub fn biome(&self, x: i32, y: i32, z: i32) -> u8 {
        match self.index(x, y, z) {
            Some(i) => self.biome[i],
            None => u8::MAX,
        }
    }

    #[inline]
    pub fn set_light(&mut self, x: i32, y: i32, z: i32, block: u8, sky: u8) {
        if let Some(i) = self.index(x, y, z) {
            self.light[i] = (block & 15) | (sky & 15) << 4;
        }
    }

    #[inline]
    pub fn fill_light_constant(&mut self, block: u8, sky: u8) {
        self.light.fill((block & 15) | (sky & 15) << 4);
    }

    #[inline]
    pub fn y_window(&self) -> (i32, i32) {
        (self.y_base, self.y_count)
    }

    #[inline]
    pub fn origin(&self) -> (i32, i32) {
        (self.x_base, self.z_base)
    }

    #[inline]
    pub fn is_full_cube(&self, x: i32, y: i32, z: i32) -> bool {
        match self.index(x, y, z) {
            Some(i) => bit_get(&self.full, i),
            None => false,
        }
    }

    #[inline]
    pub fn block_light(&self, x: i32, y: i32, z: i32) -> u8 {
        match self.index(x, y, z) {
            Some(i) => self.light[i] & 15,
            None => 0,
        }
    }

    #[inline]
    pub fn sky_light(&self, x: i32, y: i32, z: i32) -> u8 {
        match self.index(x, y, z) {
            Some(i) => self.light[i] >> 4,
            None => 15,
        }
    }

    #[inline]
    pub fn emission(&self, x: i32, y: i32, z: i32) -> u8 {
        match self.index(x, y, z) {
            Some(i) => self.emission[i],
            None => 0,
        }
    }

    #[inline]
    pub fn set_fluid(&mut self, x: i32, y: i32, z: i32, amount: u8, lava: bool) {
        if let Some(i) = self.index(x, y, z) {
            self.fluid[i] = amount | if lava { LAVA_BIT } else { 0 };
        }
    }

    #[inline]
    pub fn same_fluid(&self, x: i32, y: i32, z: i32, lava: bool) -> bool {
        match self.index(x, y, z) {
            Some(i) => self.fluid[i] != 0 && (self.fluid[i] & LAVA_BIT != 0) == lava,
            None => false,
        }
    }

    #[inline]
    pub fn is_solid(&self, x: i32, y: i32, z: i32) -> bool {
        match self.index(x, y, z) {
            Some(i) => bit_get(&self.solid, i),
            None => false,
        }
    }

    #[inline]
    pub fn same_cull_group(&self, x: i32, y: i32, z: i32, group: u32) -> bool {
        if group == 0 {
            return false;
        }
        match self.index(x, y, z) {
            Some(i) => self.cull_group[i] == group,
            None => false,
        }
    }

    #[inline]
    pub fn fluid_amount(&self, x: i32, y: i32, z: i32) -> u8 {
        match self.index(x, y, z) {
            Some(i) => self.fluid[i] & !LAVA_BIT,
            None => 0,
        }
    }

    #[inline]
    pub fn fluid_own_height(&self, x: i32, y: i32, z: i32) -> f32 {
        let amount = self.fluid_amount(x, y, z);
        if amount == 0 {
            0.0
        } else {
            super::fluid_surface(amount)
        }
    }

    #[inline]
    pub fn enclosed(&self, x: i32, y: i32, z: i32) -> bool {
        self.is_solid(x, y + 1, z)
            && self.is_solid(x, y - 1, z)
            && self.is_solid(x + 1, y, z)
            && self.is_solid(x - 1, y, z)
            && self.is_solid(x, y, z + 1)
            && self.is_solid(x, y, z - 1)
    }
}

#[inline]
fn bit_get(bits: &[u64], i: usize) -> bool {
    bits[i >> 6] & (1u64 << (i & 63)) != 0
}

#[inline]
fn bit_set(bits: &mut [u64], i: usize) {
    bits[i >> 6] |= 1u64 << (i & 63);
}

#[cfg(test)]
mod blend_tests {
    use super::*;

    fn colour(b: u8) -> [f32; 3] {
        [f32::from(b), 0.0, 0.0]
    }

    #[test]
    fn blending_averages_across_a_biome_edge() {
        let mut occ = Occupancy::new(0, 0, 64, 16);
        // Biome 0 west of x = 8, biome 4 from it.
        for qx in -2..6 {
            for qz in -2..6 {
                for qy in 16..20 {
                    occ.set_quart(qx, qy, qz, if qx < 2 { 0 } else { 4 });
                }
            }
        }
        occ.set_biome(7, 70, 5, 0);
        // Radius 0: the block's own biome.
        assert_eq!(occ.blend(7, 70, 5, 0, colour)[0], 0.0);
        // Radius 2 at x = 7: x 5..=9, two of the five columns in biome 4.
        let blended = occ.blend(7, 70, 5, 2, colour)[0];
        assert!((blended - 4.0 * 2.0 / 5.0).abs() < 1e-5, "{blended}");
        // Far from the edge nothing changes.
        assert_eq!(occ.blend(1, 70, 5, 2, colour)[0], 0.0);
    }
}
