// SPDX-License-Identifier: GPL-3.0-only
// Independent raw admission BEFORE pinned factories allocate either source array.
// This decodes fields only; handles/basis counts do not prove usable channels.
#include "spline_classes.hpp"
static void spline_target(std::string_view bytes, size_t offset, const RawHeader& header, const char* expected) {
    source_link(bytes, offset, header.sizes.size());
    const auto target = at(bytes, offset); if (target == UINT32_MAX) return;
    const auto& name = header.types[header.indices[target]];
    if (name != expected && std::binary_search(std::begin(spline_known_classes),std::end(spline_known_classes),std::string_view(name)))
        throw std::runtime_error("native spline link has wrong target kind");
}
struct SplineCounts { uint32_t floats = 0, compact = 0; };
static bool selected_spline(const std::string& name) {
    return name == "NiBSplineCompTransformInterpolator" || name == "NiBSplineData" || name == "NiBSplineBasisData";
}
static void spline_work(size_t& remaining, size_t amount) {
    if (amount > remaining) throw std::runtime_error("native spline-source work budget exceeded");
    remaining -= amount;
}
static SplineCounts preflight_spline(std::string_view bytes, const std::string& name, const RawHeader& header, size_t& remaining, size_t& work) {
    spline_work(work, 1); Scan input{bytes}; SplineCounts counts;
    if (name == "NiBSplineData") {
        storage(remaining, 1, sizeof(NiBSplineData));
        spline_work(work, 1); counts.floats = input.integer(); input.count(counts.floats, 4);
        spline_work(work, counts.floats); storage(remaining, counts.floats, sizeof(float));
        for (uint32_t i = 0; i < counts.floats; ++i) { finite_at(bytes, input.position); input.take(4); }
        spline_work(work, 1); counts.compact = input.integer(); input.count(counts.compact, 2);
        spline_work(work, counts.compact); storage(remaining, counts.compact, sizeof(short));
        input.take(size_t(counts.compact) * 2);
    } else if (name == "NiBSplineBasisData") {
        storage(remaining, 1, sizeof(NiBSplineBasisData)); spline_work(work, 1); input.take(4);
    } else if (name == "NiBSplineCompTransformInterpolator") {
        if (bytes.size() != 84) throw std::runtime_error("native compact transform source span differs");
        storage(remaining, 1, sizeof(NiBSplineCompTransformInterpolator)); spline_work(work, 21);
        spline_target(bytes, 8, header, "NiBSplineData"); spline_target(bytes, 12, header, "NiBSplineBasisData");
        for (size_t offset = 0; offset < 8; offset += 4) finite_at(bytes, offset);
        for (size_t offset = 16; offset < 48; offset += 4) finite_at(bytes, offset);
        for (size_t offset = 60; offset < 84; offset += 4) finite_at(bytes, offset);
        input.take(84);
    } else throw std::runtime_error("unadmitted native spline source class");
    input.end(); return counts;
}
static void project_spline(std::ostream& out, NiObject& block, const std::string& name, SplineCounts raw) {
    if (name == "NiBSplineData") {
        const auto data = dynamic_cast<NiBSplineData*>(&block);
        if (!data || data->floatControlPoints.size() != raw.floats || data->shortControlPoints.size() != raw.compact)
            throw std::runtime_error("native spline control-point counts differ");
        out << "{\"kind\":\"control_points\",\"declared_float_count\":" << raw.floats << ",\"float_bits\":[";
        for (uint32_t i = 0; i < raw.floats; ++i) { if (i) out << ','; out << bits(data->floatControlPoints[i]); }
        out << "],\"declared_compact_count\":" << raw.compact << ",\"compact\":[";
        for (uint32_t i = 0; i < raw.compact; ++i) { if (i) out << ','; out << data->shortControlPoints[i]; }
        out << "]}";
    } else if (name == "NiBSplineBasisData") {
        const auto basis = dynamic_cast<NiBSplineBasisData*>(&block);
        if (!basis) throw std::runtime_error("native spline basis factory differs");
        out << "{\"kind\":\"basis\",\"num_control_points\":" << basis->numControlPoints << '}';
    } else {
        const auto c = dynamic_cast<NiBSplineCompTransformInterpolator*>(&block);
        if (!c) throw std::runtime_error("native compact transform factory differs");
        out << "{\"kind\":\"compact_transform\",\"start_bits\":" << bits(c->startTime) << ",\"stop_bits\":" << bits(c->stopTime);
        field(out,"spline_data",c->splineDataRef.index); field(out,"basis_data",c->basisDataRef.index);
        out << ",\"translation_bits\":"; vec(out,c->translation);
        out << ",\"rotation_wxyz_bits\":[" << bits(c->rotation.w) << ',' << bits(c->rotation.x) << ',' << bits(c->rotation.y) << ',' << bits(c->rotation.z) << ']';
        out << ",\"scale_bits\":" << bits(c->scale) << ",\"translation_handle\":" << c->translationOffset;
        out << ",\"rotation_handle\":" << c->rotationOffset << ",\"scale_handle\":" << c->scaleOffset;
        out << ",\"translation_offset_bits\":" << bits(c->translationBias) << ",\"translation_half_range_bits\":" << bits(c->translationMultiplier);
        out << ",\"rotation_offset_bits\":" << bits(c->rotationBias) << ",\"rotation_half_range_bits\":" << bits(c->rotationMultiplier);
        out << ",\"scale_offset_bits\":" << bits(c->scaleBias) << ",\"scale_half_range_bits\":" << bits(c->scaleMultiplier) << '}';
    }
}
