// SPDX-License-Identifier: GPL-3.0-only
// Exact five-byte raw admission before pinned uint8 Boolean source factories.
static bool selected_boolean(const std::string& name) {
    return name == "NiBoolInterpolator" || name == "NiBoolTimelineInterpolator";
}
static void preflight_boolean(std::string_view bytes, const std::string& name, const RawHeader& header, size_t& remaining, size_t& work) {
    if (!selected_boolean(name) || bytes.size() != 5) throw std::runtime_error("native Boolean interpolator source span differs");
    if (work < 5) throw std::runtime_error("native Boolean source work budget exceeded");
    work -= 5;
    storage(remaining, 1, name == "NiBoolInterpolator" ? sizeof(NiBoolInterpolator) : sizeof(NiBoolTimelineInterpolator));
    source_link(bytes, 1, header.sizes.size());
    const auto target = at(bytes, 1);
    if (target != UINT32_MAX) {
        const auto& kind = header.types[header.indices[target]];
        if (kind != "NiBoolData" && std::binary_search(std::begin(spline_known_classes), std::end(spline_known_classes), std::string_view(kind)))
            throw std::runtime_error("native Boolean data link has wrong target kind");
    }
    // Any uint8 value, including the source sentinel 2, remains raw.
}
static void project_boolean(std::ostream& out, NiObject& block) {
    const auto value = dynamic_cast<NiBoolInterpolator*>(&block);
    if (!value) throw std::runtime_error("native Boolean interpolator factory differs");
    out << "{\"raw_value\":" << unsigned(value->boolValue);
    field(out, "data", value->dataRef.index);
    out << '}';
}
