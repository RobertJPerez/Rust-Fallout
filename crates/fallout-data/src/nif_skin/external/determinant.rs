//! Outward binary64 intervals certify only the newly explicit external mapping.
use super::pose::{Affine, Budget};
use crate::Result;

#[derive(Clone, Copy)]
struct Interval {
    lo: f64,
    hi: f64,
}
fn adjacent(value: f64, up: bool) -> f64 {
    if value == 0. {
        return if up {
            f64::from_bits(1)
        } else {
            -f64::from_bits(1)
        };
    }
    let bits = value.to_bits();
    f64::from_bits(if up == (value > 0.) {
        bits + 1
    } else {
        bits - 1
    })
}
impl Interval {
    fn point(value: f64) -> Self {
        Self {
            lo: value,
            hi: value,
        }
    }
    fn outward(lo: f64, hi: f64) -> Option<Self> {
        if !lo.is_finite() || !hi.is_finite() {
            return None;
        }
        let lo = adjacent(lo, false);
        let hi = adjacent(hi, true);
        (lo.is_finite() && hi.is_finite()).then_some(Self { lo, hi })
    }
    fn product(self, other: Self) -> Option<Self> {
        let corners = [
            self.lo * other.lo,
            self.lo * other.hi,
            self.hi * other.lo,
            self.hi * other.hi,
        ];
        if !corners.iter().all(|v| v.is_finite()) {
            return None;
        }
        Self::outward(
            corners.iter().copied().fold(f64::INFINITY, f64::min),
            corners.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        )
    }
    fn add(self, other: Self) -> Option<Self> {
        Self::outward(self.lo + other.lo, self.hi + other.hi)
    }
    fn subtract(self, other: Self) -> Option<Self> {
        Self::outward(self.lo - other.hi, self.hi - other.lo)
    }
}
fn enclosure(matrix: Affine) -> Option<Interval> {
    let [[a, b, c, _], [d, e, f, _], [g, h, i, _]] = matrix.map(|row| row.map(Interval::point));
    let x = e.product(i)?.subtract(f.product(h)?)?;
    let y = d.product(i)?.subtract(f.product(g)?)?;
    let z = d.product(h)?.subtract(e.product(g)?)?;
    a.product(x)?.subtract(b.product(y)?)?.add(c.product(z)?)
}

pub(super) fn certify(matrix: Affine, budget: &mut Budget<'_>) -> Result<()> {
    // Nine bounded interval products plus five additions/subtractions, with
    // outward neighbors after every rounded primitive. Subnormal results are
    // enclosed too; overflow/nonfinite bounds and intervals containing0 refuse.
    // There is no scale epsilon or rounded !=0 determinant shortcut.
    budget.charge(14)?;
    if enclosure(matrix).is_some_and(|det| det.lo > 0. || det.hi < 0.) {
        Ok(())
    } else {
        Err(budget.fail("external root-space mapping is singular or its determinant cannot be certified nonzero"))
    }
}
