// An independent projection of selected xEdit FNV layouts, not an xEdit build.
// Input is a four-byte record kind followed by a strictly decoded source body.
// This offline tool deliberately does not certify Rust's decompression or linking.
#include <windows.h>
#include <bcrypt.h>
#include <algorithm>
#include <cmath>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <limits>
#include <map>
#include <optional>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

using Bytes = std::vector<unsigned char>;
using Object = std::map<std::string, std::string>;

static std::string quote(const std::string& value) {
    std::ostringstream out; out << std::quoted(value); return out.str();
}
static std::string object(const Object& values) {
    std::string out = "{"; bool comma = false;
    for (const auto& item : values) {
        if (comma) out += ','; comma = true;
        out += quote(item.first) + ':' + item.second;
    }
    return out + '}';
}
static std::string array(const std::vector<std::string>& values) {
    std::string out = "["; bool comma = false;
    for (const auto& value : values) { if (comma) out += ','; comma = true; out += value; }
    return out + ']';
}
static std::string byte_array(const Bytes& bytes, size_t begin, size_t end, bool signed_bytes = false) {
    std::vector<std::string> out;
    for (size_t i = begin; i < end; ++i) {
        int value = bytes.at(i);
        if (signed_bytes && value >= 128) value -= 256;
        out.push_back(std::to_string(value));
    }
    return array(out);
}
static uint32_t number(const Bytes& bytes, size_t at, size_t width) {
    uint32_t out = 0;
    for (size_t i = 0; i < width; ++i) out |= uint32_t(bytes.at(at + i)) << (8 * i);
    return out;
}
static int64_t signed_number(const Bytes& bytes, size_t at, size_t width) {
    const uint32_t value = number(bytes, at, width);
    return (value & (uint32_t(1) << (width * 8 - 1))) ? int64_t(value) - (int64_t(1) << (width * 8)) : value;
}
static std::string float_bits(const Bytes& bytes, size_t at) {
    const uint32_t bits = number(bytes, at, 4); float value;
    std::memcpy(&value, &bits, 4);
    if (!std::isfinite(value)) throw std::runtime_error("nonfinite source field");
    return std::to_string(bits);
}
static std::string signature(const Bytes& bytes, size_t at) {
    std::ostringstream out;
    for (size_t i = 0; i < 4; ++i) {
        const auto b = bytes.at(at + i);
        if (b >= 33 && b <= 126) out << char(b);
        else out << "\\x" << std::hex << std::uppercase << std::setw(2) << std::setfill('0') << unsigned(b);
    }
    return out.str();
}
static Bytes read_file(const std::filesystem::path& path) {
    std::ifstream in(path, std::ios::binary | std::ios::ate);
    const auto size = in.tellg();
    if (!in || size < 0 || size > 64 * 1024 * 1024 + 4) throw std::runtime_error("input size budget");
    Bytes bytes(static_cast<size_t>(size)); in.seekg(0);
    in.read(reinterpret_cast<char*>(bytes.data()), size);
    if (!in) throw std::runtime_error("input read failed");
    return bytes;
}
static std::string digest(const Bytes& bytes) {
    BCRYPT_ALG_HANDLE algorithm = nullptr;
    if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) < 0)
        throw std::runtime_error("SHA256 provider");
    unsigned char hash[32]{};
    const auto result = BCryptHash(algorithm, nullptr, 0, const_cast<PUCHAR>(bytes.data()),
        static_cast<ULONG>(bytes.size()), hash, sizeof(hash));
    BCryptCloseAlgorithmProvider(algorithm, 0);
    if (result < 0) throw std::runtime_error("SHA256 hash");
    std::ostringstream out; out << std::hex << std::setfill('0');
    for (auto b : hash) out << std::setw(2) << unsigned(b);
    return out.str();
}
static std::string field(size_t offset, const std::string& value) {
    return object({{"decoded_offset", std::to_string(offset)}, {"value", value}});
}
static Object empty_fields(const std::string& kind) {
    Object out;
    std::vector<std::string> names;
    if (kind == "WRLD") names = {"editor_id", "full_name", "flags", "parent", "parent_flags",
        "climate", "water", "lod_water", "lod_water_height_bits", "default_height_bits",
        "image_space", "encounter_zone", "music"};
    else if (kind == "CELL") names = {"editor_id", "full_name", "flags", "grid", "quadrant_flags"};
    else if (kind == "LAND") { names = {"flags", "normals", "heights", "colors"}; out["layers"] = "[]"; }
    else throw std::runtime_error("unknown record kind");
    for (const auto& name : names) out[name] = "null";
    out["unhandled"] = "[]"; return out;
}
// Evaluate each sample from its vertical and horizontal dependency paths. This
// deliberately does not share the runtime's single-pass row accumulator.
static std::string height_grid(const Bytes& bytes, size_t start) {
    const uint32_t offset_bits = number(bytes, start, 4);
    float offset; std::memcpy(&offset, &offset_bits, 4);
    if (!std::isfinite(offset)) throw std::runtime_error("nonfinite height offset");
    std::vector<std::string> values;
    float minimum = std::numeric_limits<float>::max();
    float maximum = std::numeric_limits<float>::lowest();
    for (size_t vertex = 0; vertex < 1089; ++vertex) {
        const size_t row = vertex / 33, column = vertex % 33;
        float value = offset;
        for (size_t y = 0; y <= row; ++y)
            value = value + static_cast<float>(signed_number(bytes, start + 4 + y * 33, 1));
        for (size_t x = 1; x <= column; ++x)
            value = value + static_cast<float>(signed_number(bytes, start + 4 + row * 33 + x, 1));
        value = value * 8.0f;
        if (!std::isfinite(value)) throw std::runtime_error("height reconstruction overflow");
        if (value < minimum) minimum = value;
        if (value > maximum) maximum = value;
        uint32_t bits; std::memcpy(&bits, &value, 4); values.push_back(std::to_string(bits));
    }
    uint32_t min_bits, max_bits;
    std::memcpy(&min_bits, &minimum, 4); std::memcpy(&max_bits, &maximum, 4);
    return object({{"model", quote("esm4-vhgt-f32-row-prefix-scale8-v1")},
        {"height_bits", array(values)}, {"minimum_bits", std::to_string(min_bits)}, {"maximum_bits", std::to_string(max_bits)}});
}

static Object project(const Bytes& tagged, std::string* heights = nullptr) {
    const auto kind = signature(tagged, 0);
    Bytes bytes(tagged.begin() + 4, tagged.end());
    Object out = empty_fields(kind);
    std::vector<std::string> unknown; std::vector<Object> layers;
    const std::map<std::string, std::string> world_links = {
        {"WNAM", "parent"}, {"CNAM", "climate"}, {"NAM2", "water"}, {"NAM3", "lod_water"},
        {"INAM", "image_space"}, {"XEZN", "encounter_zone"}, {"ZNAM", "music"}};
    std::optional<size_t> extended;
    size_t pos = 0;
    while (pos < bytes.size()) {
        if (bytes.size() - pos < 6) throw std::runtime_error("short subrecord");
        const size_t offset = pos; const auto sig = signature(bytes, pos);
        size_t count = number(bytes, pos + 4, 2); pos += 6;
        if (sig == "XXXX") {
            if (count != 4 || extended || bytes.size() - pos < 4) throw std::runtime_error("bad XXXX");
            extended = number(bytes, pos, 4); pos += 4; continue;
        }
        if (extended) { count = *extended; extended.reset(); }
        if (count > bytes.size() - pos) throw std::runtime_error("subrecord overrun");
        const size_t start = pos; pos += count;
        const auto require = [&](size_t size) { if (count != size) throw std::runtime_error("field size " + sig); };
        const auto set = [&](const std::string& name, const std::string& value) {
            if (out.at(name) != "null") throw std::runtime_error("duplicate " + sig);
            out[name] = field(offset, value);
        };
        if ((kind == "WRLD" || kind == "CELL") && (sig == "EDID" || sig == "FULL")) {
            if (count == 0 || bytes[start + count - 1] != 0
                || std::find(bytes.begin() + start, bytes.begin() + start + count - 1, 0) != bytes.begin() + start + count - 1)
                throw std::runtime_error("string terminator");
            set(sig == "EDID" ? "editor_id" : "full_name", byte_array(bytes, start, start + count - 1));
        } else if (sig == "DATA") {
            require(kind == "LAND" ? 4 : 1); set("flags", std::to_string(number(bytes, start, count)));
        } else if (kind == "WRLD" && world_links.count(sig)) {
            require(4); set(world_links.at(sig), std::to_string(number(bytes, start, 4)));
        } else if (kind == "WRLD" && sig == "PNAM") {
            require(2); set("parent_flags", std::to_string(number(bytes, start, 2)));
        } else if (kind == "WRLD" && sig == "NAM4") {
            require(4); set("lod_water_height_bits", float_bits(bytes, start));
        } else if (kind == "WRLD" && sig == "DNAM") {
            require(8); set("default_height_bits", array({float_bits(bytes, start), float_bits(bytes, start + 4)}));
        } else if (kind == "CELL" && sig == "XCLC") {
            if (count != 8 && count != 12) throw std::runtime_error("grid size");
            set("grid", array({std::to_string(signed_number(bytes, start, 4)), std::to_string(signed_number(bytes, start + 4, 4))}));
            if (count == 12) set("quadrant_flags", std::to_string(number(bytes, start + 8, 4)));
        } else if (kind == "LAND" && (sig == "VNML" || sig == "VCLR")) {
            require(3267); std::vector<std::string> vectors;
            for (size_t i = 0; i < count; i += 3) vectors.push_back(byte_array(bytes, start + i, start + i + 3));
            set(sig == "VNML" ? "normals" : "colors", array(vectors));
        } else if (kind == "LAND" && sig == "VHGT") {
            require(1096);
            if (heights) *heights = height_grid(bytes, start);
            set("heights", object({{"offset_bits", float_bits(bytes, start)},
                {"deltas", byte_array(bytes, start + 4, start + 1093, true)},
                {"unused", byte_array(bytes, start + 1093, start + 1096)}}));
        } else if (kind == "LAND" && (sig == "BTXT" || sig == "ATXT")) {
            require(8); if (bytes[start + 4] > 3) throw std::runtime_error("quadrant");
            layers.push_back({{"kind", quote(sig)}, {"decoded_offset", std::to_string(offset)},
                {"texture_raw", std::to_string(number(bytes, start, 4))},
                {"quadrant", std::to_string(bytes[start + 4])}, {"unused", std::to_string(bytes[start + 5])},
                {"layer", std::to_string(signed_number(bytes, start + 6, 2))}, {"alpha", "null"}});
        } else if (kind == "LAND" && sig == "VTXT") {
            if (layers.empty() || layers.back().at("kind") != quote("ATXT") || layers.back().at("alpha") != "null" || count % 8)
                throw std::runtime_error("alpha layer framing");
            std::vector<std::string> vertices;
            for (size_t i = 0; i < count; i += 8) {
                const auto position = number(bytes, start + i, 2);
                if (position > 288) throw std::runtime_error("alpha position");
                vertices.push_back(object({{"position", std::to_string(position)},
                    {"unused", byte_array(bytes, start + i + 2, start + i + 4)},
                    {"opacity_bits", float_bits(bytes, start + i + 4)}}));
            }
            layers.back()["alpha"] = field(offset, array(vertices));
        } else {
            unknown.push_back(object({{"kind", quote(sig)}, {"decoded_offset", std::to_string(offset)},
                {"bytes", byte_array(bytes, start, start + count)}}));
        }
    }
    if (extended) throw std::runtime_error("orphan XXXX");
    out["unhandled"] = array(unknown);
    if (kind == "LAND") {
        std::vector<std::string> values; for (const auto& layer : layers) values.push_back(object(layer));
        out["layers"] = array(values);
    }
    return out;
}

int main(int argc, char** argv) {
    const bool heights = argc == 3 && std::string(argv[2]) == "--heights";
    if (argc != 2 && !heights) { std::cerr << "usage: terrain-oracle BODY_CACHE_DIRECTORY [--heights]\n"; return 2; }
    try {
        std::vector<std::filesystem::path> paths;
        for (const auto& entry : std::filesystem::directory_iterator(argv[1]))
            if (entry.is_regular_file() && entry.path().extension() == ".blob") paths.push_back(entry.path());
        std::sort(paths.begin(), paths.end());
        if (paths.empty()) throw std::runtime_error("empty input set");
        std::vector<std::string> files;
        for (const auto& path : paths) {
            const auto bytes = read_file(path);
            if (bytes.size() < 4) throw std::runtime_error("missing record tag");
            std::string grid = "null";
            const auto fields = project(bytes, heights ? &grid : nullptr);
            Object row = {{"file", quote(path.filename().string())}, {"sha256", quote(digest(bytes))}, {"fields", object(fields)}};
            if (heights) row["height_grid"] = grid;
            files.push_back(object(row));
        }
        std::cout << object({{"oracle_binary_sha256", quote(digest(read_file(argv[0])))},
            {"files", array(files)}, {"scope", quote("Authored independent field projection from tagged decoded bodies; decompression, overrides and gameplay are not compared")}}) << '\n';
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "terrain-oracle: " << error.what() << '\n'; return 1;
    }
}
