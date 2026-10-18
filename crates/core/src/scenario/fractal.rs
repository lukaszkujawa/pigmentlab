const SMALLEST_BOX: usize = 2;
const PAINTED_HEIGHT: f32 = 0.05;

pub fn box_counting_dimension(heights: &[f32], width: usize, height: usize) -> Option<f64> {
    let painted: Vec<bool> = heights.iter().map(|h| *h > PAINTED_HEIGHT).collect();
    let largest = width.min(height) / 2;
    let sizes = std::iter::successors(Some(SMALLEST_BOX), |size| Some(size * 2)).take_while(|size| *size <= largest);
    let points: Vec<(f64, f64)> = sizes.map(|size| ((1.0 / size as f64).ln(), (occupied_boxes(&painted, width, height, size) as f64).ln())).filter(|(_, count)| count.is_finite()).collect();
    if points.len() < 2 {
        return None;
    }
    let count = points.len() as f64;
    let (mean_x, mean_y) = points.iter().fold((0.0, 0.0), |(x, y), (px, py)| (x + px / count, y + py / count));
    let (covariance, variance) = points.iter().fold((0.0, 0.0), |(c, v), (x, y)| (c + (x - mean_x) * (y - mean_y), v + (x - mean_x).powi(2)));
    Some(covariance / variance)
}

fn occupied_boxes(painted: &[bool], width: usize, height: usize, size: usize) -> usize {
    let columns = width.div_ceil(size);
    let mut occupied = vec![false; columns * height.div_ceil(size)];
    for (index, _) in painted.iter().enumerate().filter(|(_, painted)| **painted) {
        occupied[(index / width / size) * columns + index % width / size] = true;
    }
    occupied.iter().filter(|box_| **box_).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIDE: usize = 512;

    fn dimension_of(painted: impl Fn(usize, usize) -> bool) -> f64 {
        let heights: Vec<f32> = (0..SIDE * SIDE).map(|i| if painted(i % SIDE, i / SIDE) { 1.0 } else { 0.0 }).collect();
        box_counting_dimension(&heights, SIDE, SIDE).expect("something is painted")
    }

    #[test]
    fn a_line_is_one_dimensional_a_filled_square_two_and_the_sierpinski_triangle_in_between() {
        let line = dimension_of(|x, y| x == y);
        let square = dimension_of(|_, _| true);
        let sierpinski = dimension_of(|x, y| x & y == 0);
        assert!((line - 1.0).abs() < 0.02, "a diagonal line measured {line:.3}");
        assert!((square - 2.0).abs() < 0.02, "a filled square measured {square:.3}");
        assert!((sierpinski - 3.0f64.log2()).abs() < 0.02, "the Sierpinski triangle measured {sierpinski:.3}, expected {:.3}", 3.0f64.log2());
    }
}
