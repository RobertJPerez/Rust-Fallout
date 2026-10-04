# Checked source coordinate boundary

`coordinates` retains the existing NV inspection conventions and source units.
`try_source_to_view(point, origin)` checks finite inputs and the rebased f64
result of `[x,y,z] -> [x,z,-y]`. Two finite inputs can subtract to Infinity;
that result refuses. `try_source_to_view_f32` additionally refuses any component
outside `[-f32::MAX, f32::MAX]` before the ordinary cast. Normal f32 rounding,
including tiny underflow, remains explicit; these helpers do not clamp or scale.

`Affine::try_point` checks source-space point products/sums. `try_relative_view`
checks the existing converted affine after f64 origin subtraction, and
`try_relative_view_f32` admits its rows only within the finite f32 range.
Rebase before narrowing so large absolute positions can retain small relative
offsets. The original unchecked functions remain for compatibility; presentation
and physics adopt the reviewed checked producer at their own clean boundaries.

Finite rows/points do not establish finite later mesh-bound products, camera
direction normalization, matrix inversion or GPU arithmetic. Those consumers
still check their own operations. This boundary introduces no measured retail
axis/unit scale, actor rotation rule, gameplay contact tolerance or source state.

Authored tests cover exact finite f32 maxima and the next binary64 value above,
finite f64 subtraction/product overflow, rebasing of extreme absolute values,
non-finite input/affine refusal and agreement with the existing source convention.
The existing height reconstructor's actual `HeightGrid::position` output is also
consumed at extreme signed CELL indices and the final cell edge. Its pinned
4096/128 height-model convention stays unchanged and remains distinct from retail
measurement; subtraction precedes narrowing so adjacent sample offsets survive.
