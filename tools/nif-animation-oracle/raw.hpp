// SPDX-License-Identifier: GPL-3.0-only
// Allocation/count guards and original header-string bytes supplement nifly.
// Framing follows the pinned XML predicates, not reconstructed writer output.
struct Scan {
    std::string_view bytes;
    size_t position = 0;
    std::string_view take(size_t count) {
        if (position > bytes.size() || count > bytes.size() - position)
            throw std::runtime_error("raw animation field exceeds source span");
        auto result = bytes.substr(position, count); position += count; return result;
    }
    uint32_t integer(size_t width = 4) {
        auto value = take(width); uint32_t result = 0;
        for (size_t i = 0; i < width; ++i) result |= uint32_t(static_cast<unsigned char>(value[i])) << (8 * i);
        return result;
    }
    void count(size_t count, size_t width, size_t maximum = 2000000) const {
        if (position > bytes.size() || count > maximum || count > (bytes.size() - position) / width)
            throw std::runtime_error("raw animation array exceeds count/span budget");
    }
    void end() const { if (position != bytes.size()) throw std::runtime_error("raw animation source has surplus bytes"); }
};
static void storage(size_t& remaining, size_t count, size_t width) {
    if (width && count > remaining / width) throw std::runtime_error("native animation storage budget exceeded");
    remaining -= count * width;
}
struct RawHeader {
    uint32_t stream = 0;
    std::vector<std::string> types, strings;
    std::vector<uint16_t> indices;
    std::vector<uint32_t> sizes;
    std::vector<size_t> offsets;
    size_t payload_start = 0, footer_offset = 0;
};
static RawHeader raw_header(const std::string& bytes) {
    RawHeader result; Scan input{bytes}; size_t remaining = 128 * 1024 * 1024;
    constexpr std::string_view prefix = "Gamebryo File Format, Version 20.2.0.7\n";
    if (input.take(prefix.size()) != prefix) throw std::runtime_error("native animation requires admitted NV header");
    const auto version = input.integer(); const auto endian = input.integer(1); const auto user = input.integer();
    const auto count = input.integer(); result.stream = input.integer();
    const std::vector<uint32_t> streams = {14,21,24,25,26,27,28,30,31,32,33,34};
    if (version != 0x14020007 || endian != 1 || user != 11 || std::find(streams.begin(), streams.end(), result.stream) == streams.end())
        throw std::runtime_error("native animation requires admitted NV tuple");
    input.count(count, 6, 100000);
    storage(remaining, count, 2 * (sizeof(uint16_t) + sizeof(uint32_t)) + sizeof(size_t) + sizeof(std::unique_ptr<NiObject>));
    for (int i = 0; i < 3; ++i) { const auto length = input.integer(1); input.take(length); }
    const auto types = input.integer(2); input.count(types, 4, 16384);
    storage(remaining, types, sizeof(std::string) + sizeof(NiString)); result.types.reserve(types);
    for (uint32_t i = 0; i < types; ++i) {
        const auto length = input.integer();
        if (!length || length > 1024) throw std::runtime_error("native animation type name exceeds budget");
        storage(remaining, length, 2); const auto raw = input.take(length);
        if (!std::all_of(raw.begin(), raw.end(), [](unsigned char c) { return c >= 0x21 && c <= 0x7e; }))
            throw std::runtime_error("native animation requires ASCII graphic type names");
        result.types.emplace_back(raw);
    }
    input.count(count, 6, 100000); result.indices.reserve(count); result.sizes.reserve(count); result.offsets.reserve(count);
    for (uint32_t i = 0; i < count; ++i) {
        const auto type = input.integer(2); if (type >= types) throw std::runtime_error("native animation type index out of range");
        result.indices.push_back(static_cast<uint16_t>(type));
    }
    size_t payload_bytes = 0;
    for (uint32_t i = 0; i < count; ++i) {
        const auto length = input.integer();
        if (length > bytes.size() - std::min(payload_bytes, bytes.size())) throw std::runtime_error("native animation payload sum exceeds input");
        payload_bytes += length; result.sizes.push_back(length);
    }
    const auto strings = input.integer(); const auto maximum_length = input.integer(); input.count(strings, 4, 1000000);
    storage(remaining, strings, sizeof(std::string) + sizeof(NiString)); result.strings.reserve(strings);
    size_t total_bytes = 0, longest = 0;
    for (uint32_t i = 0; i < strings; ++i) {
        const auto length = input.integer();
        if (length > maximum_length || length > 16 * 1024 * 1024 || length > 16 * 1024 * 1024 - total_bytes)
            throw std::runtime_error("native animation raw string budget exceeded");
        total_bytes += length; longest = std::max(longest, size_t(length));
        storage(remaining, length, 2); result.strings.emplace_back(input.take(length));
    }
    // GetStringById returns one temporary copy while checking interpretation.
    storage(remaining, longest, 1);
    const auto groups = input.integer(); input.count(groups, 4, 1000000); storage(remaining, groups, sizeof(uint32_t)); input.take(size_t(groups) * 4);
    result.payload_start = input.position;
    for (const auto size : result.sizes) { result.offsets.push_back(input.position); input.take(size); }
    result.footer_offset = input.position;
    const auto roots = input.integer(); input.count(roots, 4, 1000000); storage(remaining, roots, sizeof(NiBlockRef<NiObject>));
    for (uint32_t i = 0; i < roots; ++i) { const auto root = input.integer(); if (root != UINT32_MAX && root >= count) throw std::runtime_error("native animation footer root out of range"); }
    input.end(); return result;
}
static uint32_t at(std::string_view data, size_t offset, size_t width = 4) { Scan field{data, offset}; return field.integer(width); }
static void finite_at(std::string_view data, size_t offset) {
    auto word = at(data, offset); float value; std::memcpy(&value, &word, 4);
    if (!std::isfinite(value)) throw std::runtime_error("nonfinite native animation source float");
}
static void source_link(std::string_view data, size_t offset, size_t count) {
    const auto value = at(data, offset); if (value != UINT32_MAX && value >= count) throw std::runtime_error("native animation source link out of range");
}
struct RawCounts { uint32_t records = 0; uint16_t notes = 0; };
static RawCounts preflight(std::string_view data, const std::string& name, const RawHeader& header, size_t& remaining) {
    RawCounts result;
    const auto blocks = header.sizes.size(); const auto strings = header.strings.size();
    if (name == "NiTransformController") {
        if (data.size() != 30) throw std::runtime_error("native transform controller span differs");
        storage(remaining, 1, sizeof(NiTransformController));
        for (size_t o : {size_t(0),size_t(22),size_t(26)}) source_link(data,o,blocks);
        for (size_t o = 6; o < 22; o += 4) finite_at(data,o);
    } else if (name == "NiTransformInterpolator") {
        if (data.size() != 36) throw std::runtime_error("native transform interpolator span differs");
        storage(remaining, 1, sizeof(NiTransformInterpolator));
        for (size_t o = 0; o < 32; o += 4) finite_at(data,o); source_link(data,32,blocks);
    } else if (name == "NiTextKeyExtraData") {
        storage(remaining, 1, sizeof(NiTextKeyExtraData)); source_link(data,0,strings); result.records = at(data,4);
        Scan scan{data,8}; scan.count(result.records,8); storage(remaining,result.records,sizeof(NiTextKey));
        for (uint32_t i=0;i<result.records;++i) { finite_at(data,scan.position); source_link(data,scan.position+4,strings); scan.take(8); }
        scan.end();
    } else if (name == "NiControllerSequence") {
        storage(remaining,1,sizeof(NiControllerSequence)); source_link(data,0,strings); result.records=at(data,4);
        Scan scan{data,12}; scan.count(result.records,29); storage(remaining,result.records,sizeof(ControllerLink));
        for (uint32_t i=0;i<result.records;++i) {
            source_link(data,scan.position,blocks); source_link(data,scan.position+4,blocks);
            for(size_t o=9;o<29;o+=4) source_link(data,scan.position+o,strings); scan.take(29);
        }
        for (size_t o : {size_t(0),size_t(12),size_t(16),size_t(20)}) finite_at(data,scan.position+o);
        source_link(data,scan.position+4,blocks); source_link(data,scan.position+24,blocks); source_link(data,scan.position+28,strings); scan.take(32);
        if(header.stream>=24 && header.stream<=28) { source_link(data,scan.position,blocks); scan.take(4); }
        else if(header.stream>28) {
            result.notes=static_cast<uint16_t>(scan.integer(2)); scan.count(result.notes,4); storage(remaining,result.notes,sizeof(NiBlockRef<BSAnimNotes>));
            for(uint16_t i=0;i<result.notes;++i) { source_link(data,scan.position,blocks); scan.take(4); }
        }
        scan.end();
    }
    return result;
}
