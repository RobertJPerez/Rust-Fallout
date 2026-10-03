// SPDX-License-Identifier: GPL-3.0-only
// Independent offline nifly factories, never NifFile::Load/PrepareData.
#include "NifFile.hpp"
#include "Skin.hpp"
#include "Geometry.hpp"
#include <windows.h>
#include <bcrypt.h>
#include <algorithm>
#include <cctype>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <vector>

using namespace nifly;

static std::string snapshot(const std::filesystem::path& path) {
    std::ifstream input(path, std::ios::binary | std::ios::ate);
    const auto length = input.tellg();
    if (!input || length < 0 || length > 64 * 1024 * 1024) throw std::runtime_error("input exceeds oracle budget");
    std::string result(static_cast<size_t>(length), '\0');
    input.seekg(0); input.read(result.data(), length);
    if (!input) throw std::runtime_error("oracle snapshot read failed");
    return result;
}
static std::string sha256(const char* data, size_t length) {
    BCRYPT_ALG_HANDLE algorithm = nullptr;
    if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) < 0)
        throw std::runtime_error("SHA256 provider failed");
    unsigned char digest[32]{};
    const auto status = BCryptHash(algorithm, nullptr, 0,
        reinterpret_cast<PUCHAR>(const_cast<char*>(data)), static_cast<ULONG>(length), digest, 32);
    BCryptCloseAlgorithmProvider(algorithm, 0);
    if (status < 0) throw std::runtime_error("SHA256 failed");
    std::ostringstream output;
    output << std::hex << std::setfill('0');
    for (auto byte : digest) output << std::setw(2) << unsigned(byte);
    return output.str();
}
static uint32_t bits(float value) { uint32_t result; std::memcpy(&result, &value, 4); return result; }
static void ref(std::ostream& out, uint32_t value) { if (value == UINT32_MAX) out << "null"; else out << value; }
static void vector_bits(std::ostream& out, const Vector3& value) {
    out << '[' << bits(value.x) << ',' << bits(value.y) << ',' << bits(value.z) << ']';
}
static void transform(std::ostream& out, const MatTransform& value) {
    out << "{\"rotation_bits\":[";
    for (int row = 0; row < 3; ++row) { if (row) out << ','; vector_bits(out, value.rotation[row]); }
    out << "],\"translation_bits\":"; vector_bits(out, value.translation);
    out << ",\"scale_bits\":" << bits(value.scale) << '}';
}
static bool supported_skin(const std::string& name) {
    return name == "NiSkinData" || name == "NiSkinInstance" || name == "BSDismemberSkinInstance";
}

// Independent source-byte checks supplement the two fields normalized by nifly.
// This scan also bounds skin counts before allocating in upstream factories.
struct RawSkin {
    uint8_t flag = 0;
    std::vector<uint16_t> vertex_counts;
};
static uint32_t raw_uint(const std::string& data, size_t offset, size_t width) {
    if (offset > data.size() || width > data.size() - offset) throw std::runtime_error("raw skin field exceeds block");
    uint32_t value = 0;
    for (size_t i = 0; i < width; ++i) value |= uint32_t(static_cast<unsigned char>(data[offset + i])) << (8 * i);
    return value;
}
static RawSkin preflight(const std::string& payload, const std::string& name) {
    RawSkin result;
    if (name == "NiSkinData") {
        const auto bones = raw_uint(payload, 52, 4);
        result.flag = static_cast<uint8_t>(raw_uint(payload, 56, 1));
        if (bones > 2000000 || bones > (payload.size() - 57) / 70) throw std::runtime_error("raw skin bone count exceeds block/budget");
        size_t offset = 57;
        for (uint32_t bone = 0; bone < bones; ++bone) {
            const auto count = static_cast<uint16_t>(raw_uint(payload, offset + 68, 2));
            result.vertex_counts.push_back(count);
            offset += 70;
            const size_t weights = result.flag ? size_t(count) * 6 : 0;
            if (offset > payload.size() || weights > payload.size() - offset) throw std::runtime_error("raw skin weight array exceeds block");
            offset += weights;
        }
        if (offset != payload.size()) throw std::runtime_error("raw skin data has surplus bytes");
    } else {
        const auto bones = raw_uint(payload, 12, 4);
        if (bones > 2000000 || bones > (payload.size() - 16) / 4) throw std::runtime_error("raw skin pointer array exceeds block/budget");
        size_t end = 16 + size_t(bones) * 4;
        if (name == "BSDismemberSkinInstance") {
            const auto parts = raw_uint(payload, end, 4);
            end += 4;
            if (parts > 2000000 || parts > (payload.size() - end) / 4) throw std::runtime_error("raw body-part array exceeds block/budget");
            end += size_t(parts) * 4;
        }
        if (end != payload.size()) throw std::runtime_error("raw skin instance has surplus bytes");
    }
    return result;
}

#include "partition.hpp"
#include "binding.hpp"

static void write_file(std::ostream& out, const std::filesystem::path& path, bool include_partitions, bool include_bindings) {
    const auto bytes = snapshot(path);
    std::istringstream input(bytes, std::ios::binary);
    NiHeader header; NiIStream stream(&input, &header); header.Get(stream);
    const auto& version = header.GetVersion();
    const std::vector<uint32_t> streams = {14,21,24,25,26,27,28,30,31,32,33,34};
    if (!input || !header.IsValid() || version.File() != V20_2_0_7 || version.User() != 11
        || std::find(streams.begin(), streams.end(), version.Stream()) == streams.end()
        || header.GetNumBlocks() > 100000) throw std::runtime_error("oracle requires bounded admitted NV tuple");
    std::vector<std::unique_ptr<NiObject>> blocks;
    std::vector<size_t> offsets;
    std::vector<RawSkin> raw;
    size_t partition_storage = 128 * 1024 * 1024;
    size_t node_storage = 64 * 1024 * 1024;
    for (uint32_t id = 0; id < header.GetNumBlocks(); ++id) {
        const auto start = input.tellg();
        if (start < 0 || static_cast<size_t>(start) > bytes.size()
            || header.GetBlockSize(id) > bytes.size() - static_cast<size_t>(start)) throw std::runtime_error("oracle block exceeds file");
        offsets.push_back(static_cast<size_t>(start));
        const auto name = header.GetBlockTypeStringById(id);
        const auto payload = bytes.substr(offsets.back(), header.GetBlockSize(id));
        const bool skin = supported_skin(name);
        const bool partition = include_partitions && name == "NiSkinPartition";
        const bool node = include_bindings && selected_node(name);
        if (node) preflight_node(payload, version.Stream(), node_storage);
        if (partition) preflight_partition(payload, partition_storage);
        raw.push_back(skin ? preflight(payload, name) : RawSkin{});
        const bool geometry = name == "NiTriShape" || name == "NiTriStrips" || name == "NiTriShapeData" || name == "NiTriStripsData";
        if (skin || geometry || partition || node) {
            std::istringstream block_input(payload, std::ios::binary);
            NiIStream block_stream(&block_input, &header);
            auto factory = NiFactoryRegister::Get().GetFactoryByName(name);
            if (!factory) throw std::runtime_error("required skin/geometry factory unavailable");
            blocks.push_back(factory->Load(block_stream));
            if (!block_input || block_input.tellg() != static_cast<std::streamoff>(payload.size()))
                throw std::runtime_error("raw factory consumed unexpected block span: " + name);
        } else blocks.push_back(nullptr);
        input.seekg(start + static_cast<std::streamoff>(header.GetBlockSize(id)));
    }
    header.GetFooter(stream);
    if (!input || input.peek() != std::char_traits<char>::eof()) throw std::runtime_error("oracle footer did not consume file");
    header.SetBlockReference(&blocks);
    out << "{\"file\":" << std::quoted(path.filename().string())
        << ",\"decoded_bytes\":" << bytes.size() << ",\"sha256\":" << std::quoted(sha256(bytes.data(), bytes.size()))
        << ",\"version\":" << static_cast<uint32_t>(version.File()) << ",\"user_version\":" << version.User()
        << ",\"bethesda_version\":" << version.Stream() << ",\"skins\":[";
    bool first = true;
    size_t presence_normalizations = 0, count_normalizations = 0;
    for (uint32_t id = 0; id < blocks.size(); ++id) {
        const auto name = header.GetBlockTypeStringById(id);
        if (!supported_skin(name)) continue;
        if (!first) out << ','; first = false;
        out << "{\"block\":" << id << ",\"block_type\":" << std::quoted(name)
            << ",\"offset\":" << offsets[id] << ",\"bytes\":" << header.GetBlockSize(id)
            << ",\"sha256\":" << std::quoted(sha256(bytes.data() + offsets[id], header.GetBlockSize(id))) << ",\"data\":";
        if (auto data = header.GetBlock<NiSkinData>(id)) {
            if (data->numBones != raw[id].vertex_counts.size() || data->hasVertWeights != (raw[id].flag ? 1 : 0))
                throw std::runtime_error("nifly/source bone or presence interpretation differs");
            presence_normalizations += data->hasVertWeights != raw[id].flag;
            out << "{\"kind\":\"skin_data\",\"transform\":"; transform(out, data->skinTransform);
            out << ",\"has_vertex_weights\":" << unsigned(raw[id].flag) << ",\"bones\":[";
            for (size_t bone = 0; bone < data->bones.size(); ++bone) {
                if (bone) out << ',';
                const auto& value = data->bones[bone];
                const auto raw_count = raw[id].vertex_counts[bone];
                if (value.numVertices != (raw[id].flag ? raw_count : 0)) throw std::runtime_error("nifly/source weight count interpretation differs");
                count_normalizations += value.numVertices != raw_count;
                out << "{\"transform\":"; transform(out, value.boneTransform);
                out << ",\"center_bits\":"; vector_bits(out, value.bounds.center);
                out << ",\"radius_bits\":" << bits(value.bounds.radius) << ",\"declared_vertices\":" << raw_count << ",\"weights\":[";
                for (size_t i = 0; i < value.vertexWeights.size(); ++i) {
                    if (i) out << ',';
                    out << "{\"vertex\":" << value.vertexWeights[i].index << ",\"weight_bits\":" << bits(value.vertexWeights[i].weight) << '}';
                }
                out << "]}";
            }
            out << "]}";
        } else if (auto instance = header.GetBlock<NiSkinInstance>(id)) {
            out << "{\"kind\":\"instance\",\"instance\":{\"data\":"; ref(out, instance->dataRef.index);
            out << ",\"partition\":"; ref(out, instance->skinPartitionRef.index);
            out << ",\"skeleton_root\":"; ref(out, instance->targetRef.index);
            out << ",\"bones\":[";
            for (uint32_t bone = 0; bone < instance->boneRefs.GetSize(); ++bone) {
                if (bone) out << ','; ref(out, instance->boneRefs.GetBlockRef(bone));
            }
            out << "],\"body_parts\":";
            if (auto dismember = header.GetBlock<BSDismemberSkinInstance>(id)) {
                out << '[';
                for (size_t i = 0; i < dismember->partitions.size(); ++i) {
                    if (i) out << ',';
                    const auto& part = dismember->partitions[i];
                    out << "{\"flags\":" << uint16_t(part.flags) << ",\"body_part\":" << part.partID << '}';
                }
                out << ']';
            } else out << "null";
            out << "}}";
        } else throw std::runtime_error("skin factory type differs");
        out << '}';
    }
    out << "],\"owners\":["; first = true;
    for (uint32_t id = 0; id < blocks.size(); ++id) {
        auto geometry = header.GetBlock<NiGeometry>(id);
        if (!geometry || !geometry->HasSkinInstance()) continue;
        if (!first) out << ','; first = false;
        const auto data_id = geometry->DataRef()->index;
        out << "{\"geometry\":" << id << ",\"instance\":" << geometry->SkinInstanceRef()->index << ",\"geometry_data\":";
        ref(out, data_id); out << ",\"vertex_count\":";
        if (auto data = header.GetBlock<NiGeometryData>(data_id)) out << data->GetNumVertices(); else out << "null";
        out << '}';
    }
    out << ']';
    if (include_partitions) {
        out << ",\"partitions\":["; first = true;
        for (uint32_t id = 0; id < blocks.size(); ++id) {
            auto partition = header.GetBlock<NiSkinPartition>(id);
            if (!partition) continue;
            if (!first) out << ','; first = false;
            out << "{\"block\":" << id << ",\"block_type\":\"NiSkinPartition\",\"offset\":" << offsets[id]
                << ",\"bytes\":" << header.GetBlockSize(id) << ",\"sha256\":"
                << std::quoted(sha256(bytes.data() + offsets[id], header.GetBlockSize(id))) << ",\"partitions\":[";
            for (size_t i = 0; i < partition->partitions.size(); ++i) {
                if (i) out << ','; partition_fields(out, partition->partitions[i]);
            }
            out << "]}";
        }
        out << ']';
    }
    if (include_bindings) write_bindings(out, header, bytes, offsets);
    out << ",\"presence_normalizations\":" << presence_normalizations << ",\"vertex_count_normalizations\":" << count_normalizations << '}';
}

int main(int argc, char** argv) {
    const bool include_bindings = argc == 3 && std::string(argv[2]) == "--include-bindings";
    const bool include_partitions = include_bindings || (argc == 3 && std::string(argv[2]) == "--include-partitions");
    if (argc != 2 && !include_partitions) { std::cerr << "usage: nif-skin-oracle INPUT_FILE_OR_DIRECTORY [--include-partitions|--include-bindings]\n"; return 2; }
    try {
        const std::filesystem::path input(argv[1]);
        std::vector<std::filesystem::path> paths;
        if (std::filesystem::is_directory(input)) {
            for (const auto& entry : std::filesystem::directory_iterator(input)) {
                if (entry.is_regular_file()) {
                    auto extension = entry.path().extension().string();
                    std::transform(extension.begin(), extension.end(), extension.begin(), [](unsigned char value) { return char(std::tolower(value)); });
                    if (extension == ".blob" || extension == ".nif" || extension == ".kf") paths.push_back(entry.path());
                }
                if (paths.size() > 10000) throw std::runtime_error("oracle file count exceeds budget");
            }
        } else paths.push_back(input);
        if (paths.empty()) throw std::runtime_error("oracle found no inputs");
        std::sort(paths.begin(), paths.end());
        auto binary = snapshot(argv[0]);
        std::cout << "{\"schema_version\":" << (include_bindings ? 3 : include_partitions ? 2 : 1);
        if (include_partitions) std::cout << ",\"partition_branch\":\"nv-canonical-flags-four-wide-or-empty\",\"raw_partition_fields_checked\":true";
        if (include_bindings) std::cout << ",\"binding_scope\":\"decoded-source-forest\",\"raw_node_fields_checked\":true,\"graph_membership_checked\":true";
        std::cout << ",\"float_encoding\":\"ieee754-binary32-bits\",\"nifly_revision\":\"cca0a770094bb962fb28ea1fec5ea903e68fda8e\","
            << "\"prepare_data_called\":false,\"raw_presence_and_vertex_counts_checked\":true,\"oracle_binary_sha256\":"
            << std::quoted(sha256(binary.data(), binary.size())) << ",\"files\":[";
        bool failed = false;
        for (size_t i = 0; i < paths.size(); ++i) {
            if (i) std::cout << ',';
            try { std::ostringstream row; write_file(row, paths[i], include_partitions, include_bindings); std::cout << row.str(); }
            catch (const std::exception& error) {
                failed = true;
                std::cout << "{\"file\":" << std::quoted(paths[i].filename().string()) << ",\"error\":" << std::quoted(error.what()) << '}';
            }
        }
        std::cout << "]}\n";
        return failed ? 1 : 0;
    } catch (const std::exception& error) { std::cerr << "skin oracle: " << error.what() << '\n'; return 1; }
}
