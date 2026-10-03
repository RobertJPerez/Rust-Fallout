// SPDX-License-Identifier: GPL-3.0-only
// NV source-only branch. Call upstream read factories after bounded preflight;
// never PrepareVertexMapsAndTriangles, ConvertStripsToTriangles or pose helpers.
#pragma once

struct PartitionPreflight {
    const std::string& payload;
    size_t offset = 0;
    size_t& storage;

    uint32_t integer(size_t width) {
        const auto value = raw_uint(payload, offset, width);
        offset += width;
        return value;
    }
    uint8_t flag() {
        const auto value = integer(1);
        if (value > 1) throw std::runtime_error("unsupported noncanonical partition presence byte");
        return static_cast<uint8_t>(value);
    }
    void reserve(size_t count, size_t width) {
        if (width && count > storage / width) throw std::runtime_error("partition storage budget exceeded");
        storage -= count * width;
    }
    void array(size_t count, size_t width) {
        if (count > 2000000 || offset > payload.size() || count > (payload.size() - offset) / width)
            throw std::runtime_error("partition array exceeds block/element budget");
        reserve(count, width);
        offset += count * width;
    }
    void four_wide(uint16_t vertices, uint16_t width, uint8_t present, size_t bytes) {
        if (!present) return;
        if (vertices && width != 4) throw std::runtime_error("unsupported nonempty partition weight width");
        array(size_t(vertices) * width, bytes);
    }
};

static void preflight_partition(const std::string& payload, size_t& storage) {
    PartitionPreflight read{payload, 0, storage};
    const auto count = read.integer(4);
    if (count > 2000000 || count > (payload.size() - read.offset) / 14)
        throw std::runtime_error("partition packet count exceeds block/element budget");
    read.reserve(count, sizeof(NiSkinPartition::PartitionBlock));
    for (uint32_t ordinal = 0; ordinal < count; ++ordinal) {
        const auto vertices = static_cast<uint16_t>(read.integer(2));
        const auto triangles = static_cast<uint16_t>(read.integer(2));
        const auto bones = static_cast<uint16_t>(read.integer(2));
        const auto strips = static_cast<uint16_t>(read.integer(2));
        const auto width = static_cast<uint16_t>(read.integer(2));
        read.array(bones, 2);
        if (read.flag()) read.array(vertices, 2);
        read.four_wide(vertices, width, read.flag(), 4);
        read.reserve(strips, sizeof(uint16_t));
        std::vector<uint16_t> lengths;
        for (uint16_t i = 0; i < strips; ++i) lengths.push_back(static_cast<uint16_t>(read.integer(2)));
        if (read.flag()) {
            if (strips) {
                read.reserve(strips, sizeof(std::vector<uint16_t>));
                size_t total = 0;
                for (auto length : lengths) total += length;
                if (total > 2000000) throw std::runtime_error("partition strip element budget exceeded");
                for (auto length : lengths) read.array(length, 2);
            } else read.array(triangles, 6);
        }
        read.four_wide(vertices, width, read.flag(), 1);
    }
    if (read.offset != payload.size()) throw std::runtime_error("partition source has surplus bytes");
}

template <class T> static void integer_array(std::ostream& out, const std::vector<T>& values) {
    out << '[';
    for (size_t i = 0; i < values.size(); ++i) { if (i) out << ','; out << unsigned(values[i]); }
    out << ']';
}

static void partition_fields(std::ostream& out, const NiSkinPartition::PartitionBlock& part) {
    out << "{\"num_vertices\":" << part.numVertices << ",\"num_triangles\":" << part.numTriangles
        << ",\"num_bones\":" << part.numBones << ",\"num_strips\":" << part.numStrips
        << ",\"weights_per_vertex\":" << part.numWeightsPerVertex << ",\"bone_palette\":";
    integer_array(out, part.bones);
    out << ",\"has_vertex_map\":" << unsigned(part.hasVertexMap) << ",\"vertex_map\":";
    integer_array(out, part.vertexMap);
    out << ",\"has_vertex_weights\":" << unsigned(part.hasVertexWeights) << ",\"weight_bits\":[";
    for (size_t i = 0; i < part.vertexWeights.size(); ++i) {
        if (i) out << ',';
        const auto& w = part.vertexWeights[i];
        out << bits(w.w1) << ',' << bits(w.w2) << ',' << bits(w.w3) << ',' << bits(w.w4);
    }
    out << "],\"strip_lengths\":"; integer_array(out, part.stripLengths);
    out << ",\"has_faces\":" << unsigned(part.hasFaces) << ",\"strips\":[";
    for (size_t i = 0; i < part.strips.size(); ++i) { if (i) out << ','; integer_array(out, part.strips[i]); }
    out << "],\"triangles\":[";
    for (size_t i = 0; i < part.triangles.size(); ++i) {
        if (i) out << ',';
        const auto& t = part.triangles[i]; out << '[' << t.p1 << ',' << t.p2 << ',' << t.p3 << ']';
    }
    out << "],\"has_bone_indices\":" << unsigned(part.hasBoneIndices) << ",\"bone_indices\":[";
    for (size_t i = 0; i < part.boneIndices.size(); ++i) {
        if (i) out << ',';
        const auto& b = part.boneIndices[i];
        out << unsigned(b.i1) << ',' << unsigned(b.i2) << ',' << unsigned(b.i3) << ',' << unsigned(b.i4);
    }
    out << "]}";
}
