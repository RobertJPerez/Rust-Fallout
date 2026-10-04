// SPDX-License-Identifier: GPL-3.0-only
// Exact count/tag/span/work admission before pinned NiBoolData key allocation.
static uint32_t preflight_boolean_keys(std::string_view bytes, size_t& remaining, size_t& work) {
    auto charge = [&](size_t units) {
        if (units > work) throw std::runtime_error("native Boolean-key work budget exceeded");
        work -= units;
    };
    charge(2); // Selected block and source count.
    Scan input{bytes}; const auto count = input.integer();
    if (count) {
        charge(1); const auto tag = input.integer();
        if (tag != 5) throw std::runtime_error("native NiBoolData key type unsupported in constant source branch");
    }
    input.count(count, 5);
    if (bytes.size() - input.position != size_t(count) * 5)
        throw std::runtime_error("native NiBoolData source has surplus bytes");
    charge(size_t(count) * 2);
    storage(remaining, 1, sizeof(NiBoolData));
    storage(remaining, count, sizeof(NiAnimationKey<uint8_t>));
    for (uint32_t i = 0; i < count; ++i) {
        finite_at(bytes, input.position); input.take(5);
    }
    input.end(); return count;
}
static void project_boolean_keys(std::ostream& out, NiObject& block, uint32_t count) {
    const auto data = dynamic_cast<NiBoolData*>(&block);
    if (!data || data->data.GetNumKeys() != count)
        throw std::runtime_error("native NiBoolData key count/factory differs");
    if (count && data->data.GetInterpolationType() != NiKeyType::CONST_KEY)
        throw std::runtime_error("native NiBoolData interpolation tag differs");
    out << "{\"declared_keys\":" << count << ",\"key_type\":";
    if (count) out << 5; else out << "null";
    out << ",\"keys\":[";
    for (uint32_t i = 0; i < count; ++i) {
        if (i) out << ',';
        const auto key = data->data.GetKey(static_cast<int>(i));
        out << "{\"time_bits\":" << bits(key.time) << ",\"raw_value\":" << unsigned(key.value) << '}';
    }
    out << "]}";
}
