//! A price graph drawn in braille: 2x4 dots per cell (port of `BrailleGraph`).
//!
//! A block sparkline gives eight levels of height in one row. A braille cell
//! addresses two dot columns and four dot rows, so a `W x H` box carries `4H`
//! levels and `2W` sample points.

/// Bit of the braille cell for each (dot column, dot row). The fourth row is
/// the 8-dot extension, hence 0x40/0x80 rather than a run.
pub const DOTS: [[u16; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

/// The empty braille cell.
pub const EMPTY: &str = "\u{2800}";

#[derive(Debug, Clone, Default)]
pub struct BrailleGraph {
    pub data: Vec<f64>,
    pub fill: bool,
}

impl BrailleGraph {
    pub fn new(data: Vec<f64>) -> Self {
        Self { data, fill: false }
    }

    pub fn filled(data: Vec<f64>) -> Self {
        Self { data, fill: true }
    }

    /// One value per dot column: the mean of its bucket, never a dropped point.
    fn sample(&self, columns: usize) -> Vec<f64> {
        let series = &self.data;
        let per = series.len() as f64 / columns as f64;
        let mut out = Vec::with_capacity(columns);
        for x in 0..columns {
            let start = (x as f64 * per) as usize;
            let end = std::cmp::max(((x + 1) as f64 * per) as usize, start + 1).min(series.len());
            if end > start {
                let chunk = &series[start..end];
                out.push(chunk.iter().sum::<f64>() / chunk.len() as f64);
            }
        }
        out
    }

    /// Linearly stretch `points` to exactly `width` values (K1).
    ///
    /// When there are fewer closes than dot columns, the bucket mean repeats
    /// each close across several columns as a flat, gappy run. Resampling
    /// instead interpolates a point per column so consecutive columns differ
    /// by at most a dot row and the line reads as continuous.
    fn resample(&self, points: &[f64], width: usize) -> Vec<f64> {
        if points.len() == width {
            return points.to_vec();
        }
        if points.len() < 2 || width < 1 {
            return points.to_vec();
        }
        let step = (points.len() - 1) as f64 / (width - 1) as f64;
        (0..width).map(|i| lerp(points, i as f64 * step)).collect()
    }

    /// One value per dot column: resampled when sparse, else the bucket mean.
    fn scaled_points(&self, dot_columns: usize) -> Vec<f64> {
        if self.data.len() > dot_columns {
            self.sample(dot_columns)
        } else {
            self.resample(&self.data, dot_columns)
        }
    }

    /// Connected `(dot column, dot row)` coordinates for the whole series.
    ///
    /// Plots one dot per column and Bresenham-joins consecutive columns so the
    /// line is continuous (K1) regardless of how many source points exist.
    fn dot_coords(
        &self,
        dot_columns: usize,
        dot_rows: usize,
        low: f64,
        span: f64,
    ) -> std::collections::BTreeSet<(usize, usize)> {
        let mut dots = std::collections::BTreeSet::new();
        let mut previous: Option<(usize, usize)> = None;
        for (column, value) in self.scaled_points(dot_columns).into_iter().enumerate() {
            // Python rounds the scaled term first, then subtracts:
            // `dot_rows - 1 - round((value - low) / span * (dot_rows - 1))`.
            let scaled = (value - low) / span * (dot_rows as f64 - 1.0);
            let row = (dot_rows as f64 - 1.0 - py_round(scaled)) as i64;
            let row = row.clamp(0, dot_rows as i64 - 1) as usize;
            match previous {
                None => {
                    dots.insert((column, row));
                }
                Some((px, py)) => bresenham(px, py, column, row, &mut dots),
            }
            previous = Some((column, row));
        }
        dots
    }

    /// Braille cell bitmaps for a connected line over `low`/`span`.
    ///
    /// Shared by [`BrailleGraph::rows`] and the chart: the connected dot
    /// coordinates (with the optional area fill) OR-ed into `columns` braille
    /// cells of `rows` cells tall.
    pub fn cell_grid(&self, columns: usize, rows: usize, low: f64, span: f64) -> Vec<Vec<u16>> {
        let dot_rows = rows * 4;
        let dot_columns = columns * 2;
        let mut dots = self.dot_coords(dot_columns, dot_rows, low, span);
        if self.fill {
            let mut column_tops: std::collections::BTreeMap<usize, usize> =
                std::collections::BTreeMap::new();
            for &(column, row) in &dots {
                let top = column_tops.entry(column).or_insert(dot_rows);
                *top = (*top).min(row);
            }
            for (column, top) in column_tops {
                for row in top..dot_rows {
                    dots.insert((column, row));
                }
            }
        }
        let mut grid = vec![vec![0u16; columns]; rows];
        for &(column, row) in &dots {
            grid[row / 4][column / 2] |= DOTS[column % 2][row % 4];
        }
        grid
    }

    /// The graph as `height` strings of `width` braille cells.
    pub fn rows(&self, width: usize, height: usize) -> Vec<String> {
        let blank = vec![EMPTY.repeat(width); height];
        if self.data.is_empty() || width < 1 || height < 1 {
            return blank;
        }
        let low = self.data.iter().cloned().fold(f64::INFINITY, f64::min);
        let high = self.data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let span = (high - low).max(f64::MIN_POSITIVE).max(0.0);
        let span = if span == 0.0 { 1.0 } else { span };
        let grid = self.cell_grid(width, height, low, span);
        grid.iter()
            .map(|row| row.iter().map(|&cell| braille_char(cell)).collect())
            .collect()
    }
}

/// Python `round()`: half-to-even (banker's rounding), not half-away-from-zero.
/// The braille dot rows hit .5 ties, so parity requires the Python semantics.
pub fn py_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor == 0.5 || floor - x == 0.5 {
        if (floor as i64) % 2 == 0 {
            floor
        } else {
            floor + 1.0
        }
    } else {
        x.round()
    }
}

/// The braille glyph for one cell bitmap.
pub fn braille_char(cell: u16) -> char {
    char::from_u32(0x2800 + cell as u32).unwrap_or(EMPTY.chars().next().unwrap())
}

/// Linear interpolation into `points` at fractional index `pos`.
fn lerp(points: &[f64], pos: f64) -> f64 {
    let low = (pos as usize).min(points.len() - 1);
    let high = (low + 1).min(points.len() - 1);
    let frac = pos - low as f64;
    points[low] + (points[high] - points[low]) * frac
}

/// Mark every dot on the integer line from (x0, y0) to (x1, y1) (K1).
fn bresenham(
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    dots: &mut std::collections::BTreeSet<(usize, usize)>,
) {
    let (mut x0, mut y0) = (x0 as i64, y0 as i64);
    let (x1, y1) = (x1 as i64, y1 as i64);
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        dots.insert((x0 as usize, y0 as usize));
        if x0 == x1 && y0 == y1 {
            return;
        }
        let double = 2 * err;
        if double >= dy {
            err += dy;
            x0 += sx;
        }
        if double <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> Vec<f64> {
        vec![10.0, 11.0, 10.5, 12.0, 11.5, 13.0, 12.5]
    }

    #[test]
    fn rows_match_python_golden() {
        let g = BrailleGraph::new(data());
        let rows = g.rows(20, 3);
        assert_eq!(rows[0], "\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2880}\u{2814}\u{2809}\u{2811}\u{2812}\u{2824}");
        assert_eq!(rows[1], "\u{2800}\u{2800}\u{2800}\u{2880}\u{2800}\u{2800}\u{2800}\u{2880}\u{2860}\u{280a}\u{2809}\u{2811}\u{2812}\u{2812}\u{2801}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}");
        assert_eq!(rows[2], "\u{2860}\u{2814}\u{280a}\u{2801}\u{2809}\u{2811}\u{2812}\u{2801}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}");
    }

    #[test]
    fn one_row_fill_matches_python_golden() {
        let g = BrailleGraph::filled(data());
        assert_eq!(
            g.rows(20, 1)[0],
            "\u{28c0}\u{28c0}\u{28e4}\u{28e4}\u{28e4}\u{28e4}\u{28c4}\u{28e4}\u{28e4}\u{28f6}\u{28f6}\u{28f6}\u{28f6}\u{28f6}\u{28f6}\u{28fe}\u{28ff}\u{28ff}\u{28ff}\u{28f7}"
        );
    }

    #[test]
    fn sparse_resample_matches_python_golden() {
        let g = BrailleGraph::new(vec![10.0, 13.0]);
        let rows = g.rows(10, 2);
        assert_eq!(
            rows[0],
            "\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{28c0}\u{2860}\u{2814}\u{2812}\u{2809}"
        );
        assert_eq!(
            rows[1],
            "\u{28c0}\u{2824}\u{2814}\u{280a}\u{2809}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}"
        );
    }

    #[test]
    fn empty_data_gives_blanks() {
        let g = BrailleGraph::new(vec![]);
        assert_eq!(g.rows(5, 2), vec![EMPTY.repeat(5), EMPTY.repeat(5)]);
    }
}
