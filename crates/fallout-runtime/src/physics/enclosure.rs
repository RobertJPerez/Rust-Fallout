//! Directed finite intervals for culling transforms. These never decide contact.
#[derive(Clone, Copy, Debug)]
pub(super) struct Interval {
    pub lower: f64,
    pub upper: f64,
}
impl Interval {
    pub fn new(lower: f64, upper: f64) -> Option<Self> {
        (lower.is_finite() && upper.is_finite() && lower <= upper).then_some(Self { lower, upper })
    }
    pub fn point(value: f64) -> Self {
        Self {
            lower: value,
            upper: value,
        }
    }
    fn is_point(self, value: f64) -> bool {
        self.lower == value && self.upper == value
    }
    pub fn negate(self) -> Self {
        Self {
            lower: -self.upper,
            upper: -self.lower,
        }
    }
    pub fn add(self, other: Self) -> Option<Self> {
        if self.is_point(0.) {
            return Some(other);
        }
        if other.is_point(0.) {
            return Some(self);
        }
        Self::new(
            (self.lower + other.lower).next_down(),
            (self.upper + other.upper).next_up(),
        )
    }
    pub fn subtract(self, other: Self) -> Option<Self> {
        self.add(other.negate())
    }
    pub fn multiply(self, other: Self) -> Option<Self> {
        if self.is_point(0.) || other.is_point(0.) {
            return Some(Self::point(0.));
        }
        if self.is_point(1.) {
            return Some(other);
        }
        if other.is_point(1.) {
            return Some(self);
        }
        if self.is_point(-1.) {
            return Some(other.negate());
        }
        if other.is_point(-1.) {
            return Some(self.negate());
        }
        let products = [
            self.lower * other.lower,
            self.lower * other.upper,
            self.upper * other.lower,
            self.upper * other.upper,
        ];
        if products.iter().any(|v| !v.is_finite()) {
            return None;
        }
        Self::new(
            products
                .into_iter()
                .fold(f64::INFINITY, f64::min)
                .next_down(),
            products
                .into_iter()
                .fold(f64::NEG_INFINITY, f64::max)
                .next_up(),
        )
    }
    pub fn divide(self, other: Self) -> Option<Self> {
        if other.lower <= 0. && other.upper >= 0. {
            return None;
        }
        if self.is_point(0.) {
            return Some(Self::point(0.));
        }
        if other.is_point(1.) {
            return Some(self);
        }
        if other.is_point(-1.) {
            return Some(self.negate());
        }
        let quotients = [
            self.lower / other.lower,
            self.lower / other.upper,
            self.upper / other.lower,
            self.upper / other.upper,
        ];
        if quotients.iter().any(|v| !v.is_finite()) {
            return None;
        }
        Self::new(
            quotients
                .into_iter()
                .fold(f64::INFINITY, f64::min)
                .next_down(),
            quotients
                .into_iter()
                .fold(f64::NEG_INFINITY, f64::max)
                .next_up(),
        )
    }
    pub fn absolute_upper(self) -> f64 {
        self.lower.abs().max(self.upper.abs())
    }
}
fn cross(a: [Interval; 3], b: [Interval; 3]) -> Option<[Interval; 3]> {
    Some([
        a[1].multiply(b[2])?.subtract(a[2].multiply(b[1])?)?,
        a[2].multiply(b[0])?.subtract(a[0].multiply(b[2])?)?,
        a[0].multiply(b[1])?.subtract(a[1].multiply(b[0])?)?,
    ])
}
/// Enclose the mathematical inverse of the ACTUAL stored binary64 matrix.
/// A zero-containing determinant or unrepresentable interval falls back.
pub(super) fn inverse(matrix: [[f64; 3]; 3]) -> Option<[[Interval; 3]; 3]> {
    if matrix.iter().flatten().any(|v| !v.is_finite()) {
        return None;
    }
    let columns: [[Interval; 3]; 3] =
        std::array::from_fn(|j| std::array::from_fn(|i| Interval::point(matrix[i][j])));
    let rows = [
        cross(columns[1], columns[2])?,
        cross(columns[2], columns[0])?,
        cross(columns[0], columns[1])?,
    ];
    let mut determinant = Interval::point(0.);
    for (column, cofactor) in columns[0].iter().zip(rows[0]) {
        determinant = determinant.add(column.multiply(cofactor)?)?;
    }
    let mut result = [[Interval::point(0.); 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            result[i][j] = rows[i][j].divide(determinant)?;
        }
    }
    Some(result)
}

/// Upper arithmetic for nonnegative error/radius coefficients. Overflow stays
/// infinite so the query retains candidates rather than excluding anything.
pub(super) fn upper_product(a: f64, b: f64) -> f64 {
    if a == 0. || b == 0. {
        return 0.;
    }
    if a == 1. {
        return b;
    }
    if b == 1. {
        return a;
    }
    (a * b).next_up()
}
pub(super) fn upper_sum(a: f64, b: f64) -> f64 {
    if a == 0. {
        return b;
    }
    if b == 0. {
        return a;
    }
    (a + b).next_up()
}
pub(super) fn upper_quotient(a: f64, b: f64) -> f64 {
    if a == 0. {
        return 0.;
    }
    if b == 1. {
        return a;
    }
    (a / b).next_up()
}
