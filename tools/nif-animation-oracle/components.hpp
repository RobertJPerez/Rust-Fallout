// SPDX-License-Identifier: GPL-3.0-only
// Exact fixed-size raw admission before pinned compact component factories.
static bool selected_component(const std::string& name) {
    return name == "NiBSplineCompFloatInterpolator" || name == "NiBSplineCompPoint3Interpolator";
}
static void preflight_component(std::string_view bytes, const std::string& name, const RawHeader& header, size_t& remaining, size_t& work) {
    const bool scalar = name == "NiBSplineCompFloatInterpolator";
    const size_t size = scalar ? 32 : 40;
    if (!selected_component(name) || bytes.size() != size) throw std::runtime_error("native compact component source span differs");
    spline_work(work, 1 + size / 4);
    storage(remaining, 1, scalar ? sizeof(NiBSplineCompFloatInterpolator) : sizeof(NiBSplineCompPoint3Interpolator));
    spline_target(bytes, 8, header, "NiBSplineData");
    spline_target(bytes, 12, header, "NiBSplineBasisData");
    const size_t handle = scalar ? 20 : 28;
    for (size_t offset = 0; offset < size; offset += 4)
        if (offset != 8 && offset != 12 && offset != handle) finite_at(bytes, offset);
}
static void component_base(std::ostream& out, const NiBSplineInterpolator& block) {
    out << ",\"start_bits\":" << bits(block.startTime) << ",\"stop_bits\":" << bits(block.stopTime);
    field(out, "spline_data", block.splineDataRef.index);
    field(out, "basis_data", block.basisDataRef.index);
}
static void project_component(std::ostream& out, NiObject& block, const std::string& name) {
    if (name == "NiBSplineCompFloatInterpolator") {
        const auto scalar = dynamic_cast<NiBSplineCompFloatInterpolator*>(&block);
        if (!scalar) throw std::runtime_error("native compact float factory differs");
        out << "{\"kind\":\"compact_float\"";
        component_base(out, *scalar);
        out << ",\"value_bits\":" << bits(scalar->base) << ",\"handle\":" << scalar->offset;
        out << ",\"float_offset_bits\":" << bits(scalar->bias) << ",\"float_half_range_bits\":" << bits(scalar->multiplier) << '}';
    } else if (name == "NiBSplineCompPoint3Interpolator") {
        const auto point = dynamic_cast<NiBSplineCompPoint3Interpolator*>(&block);
        if (!point) throw std::runtime_error("native compact point3 factory differs");
        out << "{\"kind\":\"compact_point3\"";
        component_base(out, *point);
        out << ",\"value_bits\":"; vec(out, point->value);
        out << ",\"handle\":" << point->handle << ",\"position_offset_bits\":" << bits(point->positionOffset);
        out << ",\"position_half_range_bits\":" << bits(point->positionHalfRange) << '}';
    } else throw std::runtime_error("unadmitted compact component projection");
}
