// SPDX-License-Identifier: GPL-3.0-only
// Raw factory loading deliberately bypasses NifFile::PrepareData. The oracle
// must not clean texture paths or remove bad triangles before we compare them.
#pragma once
#include "NifFile.hpp"
#include "materials.hpp"
#include <fstream>
#include <cmath>
#include <cstring>
#include <functional>
#include <iomanip>
#include <iostream>
#include <limits>
#include <stdexcept>
#include <string>

namespace scene_oracle {
using namespace nifly;

inline void ref(uint32_t index) {
    if (index == UINT32_MAX) std::cout << "null";
    else std::cout << index;
}
template<class Range, class Emit> void array(const Range& range, Emit emit) {
    std::cout << '[';
    bool first = true;
    for (auto it = range.cbegin(); it != range.cend(); ++it) {
        if (!first) std::cout << ',';
        first = false;
        emit(*it);
    }
    std::cout << ']';
}
template<class T> void refs(const NiBlockRefArray<T>& values) {
    std::cout << '[';
    bool first = true;
    for (auto i = values.cbegin(); i != values.cend(); ++i) {
        if (!first) std::cout << ',';
        first = false;
        ref(i->index);
    }
    std::cout << ']';
}
inline void vector3(const Vector3& v) { std::cout << '[' << v.x << ',' << v.y << ',' << v.z << ']'; }
inline void triangle(const Triangle& v) { std::cout << '[' << v.p1 << ',' << v.p2 << ',' << v.p3 << ']'; }
inline void transform(const MatTransform& t) {
    std::cout << "{\"translation\":";
    vector3(t.translation);
    std::cout << ",\"rotation\":[";
    for (int row = 0; row < 3; ++row) {
        if (row) std::cout << ',';
        vector3(t.rotation[row]);
    }
    std::cout << "],\"scale\":" << t.scale << '}';
}

inline void write(const std::filesystem::path& path, bool diagnostics = false) {
    std::ifstream input(path, std::ios::binary);
    if (!input) throw std::runtime_error("cannot open oracle input");
    NiHeader header;
    NiIStream stream(&input, &header);
    header.Get(stream);
    if (!header.IsValid() || header.GetVersion().File() != V20_2_0_7 || header.GetVersion().User() != 11)
        throw std::runtime_error("raw scene oracle requires NV container tuple");
    std::vector<std::unique_ptr<NiObject>> blocks;
    for (uint32_t id = 0; id < header.GetNumBlocks(); ++id) {
        const auto start = input.tellg();
        const auto name = header.GetBlockTypeStringById(id);
        auto factory = NiFactoryRegister::Get().GetFactoryByName(name);
        if (factory) blocks.push_back(factory->Load(stream));
        else blocks.push_back(std::make_unique<NiUnknown>(stream, header.GetBlockSize(id)));
        if (!input || input.tellg() - start != header.GetBlockSize(id))
            throw std::runtime_error("oracle factory consumed an unexpected block size: " + name);
    }
    header.GetFooter(stream);
    if (!input || input.peek() != std::char_traits<char>::eof())
        throw std::runtime_error("oracle footer did not consume file");
    header.SetBlockReference(&blocks);
    if (diagnostics) {
        std::cout << ",\"raw_scene_issues\":[";
        bool first_issue = true;
        auto issue = [&](uint32_t block, const char* field, size_t element, size_t component, uint32_t value, const char* reason) {
            if (!first_issue) std::cout << ',';
            first_issue = false;
            std::cout << "{\"block\":" << block << ",\"field\":" << std::quoted(field)
                      << ",\"element\":" << element << ",\"component\":" << component
                      << ",\"value_bits_or_index\":" << value << ",\"reason\":" << std::quoted(reason) << '}';
        };
        for (uint32_t id = 0; id < blocks.size(); ++id) {
            auto finite = [&](float value, const char* field, size_t element, size_t component) {
                if (std::isfinite(value)) return;
                uint32_t bits; std::memcpy(&bits, &value, sizeof(bits));
                issue(id, field, element, component, bits, "nonfinite float");
            };
            if (auto av = header.GetBlock<NiAVObject>(id)) {
                for (int i = 0; i < 3; ++i) {
                    finite(av->transform.translation[i], "translation", 0, i);
                    for (int j = 0; j < 3; ++j) finite(av->transform.rotation[i][j], "rotation", i, j);
                }
                finite(av->transform.scale, "scale", 0, 0);
            }
            auto data = header.GetBlock<NiGeometryData>(id);
            if (!data) continue;
            auto vectors = [&](const auto& values, const char* field) {
                for (size_t i = 0; i < values.size(); ++i)
                    for (size_t j = 0; j < 3; ++j) finite(values[i][j], field, i, j);
            };
            vectors(data->vertices, "vertices"); vectors(data->normals, "normals");
            vectors(data->tangents, "tangents"); vectors(data->bitangents, "bitangents");
            const auto bound = data->GetBounds();
            for (int i = 0; i < 3; ++i) finite(bound.center[i], "bound", 0, i);
            finite(bound.radius, "bound", 0, 3);
            for (size_t i = 0; i < data->vertexColors.size(); ++i) {
                const auto c = data->vertexColors[i];
                finite(c.r, "colors", i, 0); finite(c.g, "colors", i, 1);
                finite(c.b, "colors", i, 2); finite(c.a, "colors", i, 3);
            }
            for (size_t set = 0; set < data->uvSets.size(); ++set)
                for (size_t i = 0; i < data->uvSets[set].size(); ++i) {
                    finite(data->uvSets[set][i].u, "uv_u", i, set);
                    finite(data->uvSets[set][i].v, "uv_v", i, set);
                }
            auto vertex = [&](uint16_t value, const char* field, size_t element, size_t component) {
                if (value >= data->GetNumVertices()) issue(id, field, element, component, value, "vertex index out of range");
            };
            std::vector<Triangle> tris; data->GetTriangles(tris);
            for (size_t i = 0; i < tris.size(); ++i) {
                vertex(tris[i].p1, "triangles", i, 0); vertex(tris[i].p2, "triangles", i, 1); vertex(tris[i].p3, "triangles", i, 2);
            }
            if (auto strips = dynamic_cast<NiTriStripsData*>(data)) {
                for (size_t i = 0; i < strips->stripsInfo.points.size(); ++i)
                    for (size_t j = 0; j < strips->stripsInfo.points[i].size(); ++j)
                        vertex(strips->stripsInfo.points[i][j], "strip_indices", i, j);
            }
            if (auto shape = dynamic_cast<NiTriShapeData*>(data)) {
                const auto groups = shape->GetMatchGroups();
                for (size_t i = 0; i < groups.size(); ++i)
                    for (size_t j = 0; j < groups[i].matches.size(); ++j)
                        vertex(groups[i].matches[j], "match_groups", i, j);
            }
        }
        std::cout << ']';
        return;
    }
    std::cout << std::showpoint << std::setprecision(std::numeric_limits<float>::max_digits10);
    std::cout << ",\"raw_scene\":{\"objects\":[";
    bool first = true;
    std::vector<uint32_t> selected;
    for (uint32_t id = 0; id < blocks.size(); ++id) {
        const auto name = header.GetBlockTypeStringById(id);
        if (name != "NiNode" && name != "BSFadeNode" && name != "NiTriShape" && name != "NiTriStrips") continue;
        auto object = header.GetBlock<NiAVObject>(id);
        if (!object) throw std::runtime_error("oracle object cast failed");
        selected.push_back(id);
        if (!first) std::cout << ',';
        first = false;
        std::cout << "{\"block\":" << id << ",\"name\":";
        ref(object->name.GetIndex());
        std::cout << ",\"extra_data\":"; refs(object->extraDataRefs);
        std::cout << ",\"controller\":"; ref(object->controllerRef.index);
        std::cout << ",\"flags\":" << object->flags << ",\"transform\":";
        transform(object->transform);
        std::cout << ",\"properties\":"; refs(object->propertyRefs);
        std::cout << ",\"collision\":"; ref(object->collisionRef.index);
        std::cout << ",\"kind\":{";
        if (auto node = dynamic_cast<NiNode*>(object)) {
            std::cout << "\"kind\":\"node\",\"children\":"; refs(node->childRefs);
            std::cout << ",\"effects\":"; refs(node->effectRefs);
        } else {
            auto geom = dynamic_cast<NiGeometry*>(object);
            std::cout << "\"kind\":\"mesh\",\"data\":"; ref(geom->DataRef()->index);
            std::cout << ",\"skin\":"; ref(geom->SkinInstanceRef()->index);
            std::cout << ",\"material_names\":";
            array(geom->materialNames, [](const NiStringRef& v) { ref(v.GetIndex()); });
            std::cout << ",\"material_extra\":";
            array(geom->materialExtraData, [](uint32_t v) { std::cout << static_cast<int32_t>(v); });
            std::cout << ",\"active_material\":" << geom->activeMaterial
                      << ",\"material_needs_update\":" << (geom->defaultMatNeedsUpdateFlag ? "true" : "false");
        }
        std::cout << "}}";
    }
    std::cout << "],\"meshes\":[";
    first = true;
    for (uint32_t id = 0; id < blocks.size(); ++id) {
        const auto name = header.GetBlockTypeStringById(id);
        if (name != "NiTriShapeData" && name != "NiTriStripsData") continue;
        auto data = header.GetBlock<NiGeometryData>(id);
        if (!first) std::cout << ',';
        first = false;
        std::cout << "{\"block\":" << id << ",\"group_id\":" << data->groupID
                  << ",\"vertex_count\":" << data->GetNumVertices()
                  << ",\"keep_flags\":" << unsigned(data->keepFlags)
                  << ",\"compress_flags\":" << unsigned(data->compressFlags)
                  << ",\"has_vertices\":" << (data->HasVertices() ? "true" : "false")
                  << ",\"vertices\":";
        array(data->vertices, vector3);
        std::cout << ",\"data_flags\":" << data->dataFlags
                  << ",\"has_normals\":" << (data->HasNormals() ? "true" : "false") << ",\"normals\":";
        array(data->normals, vector3);
        std::cout << ",\"tangents\":"; array(data->tangents, vector3);
        std::cout << ",\"bitangents\":"; array(data->bitangents, vector3);
        const auto bound = data->GetBounds();
        std::cout << ",\"bound\":[" << bound.center.x << ',' << bound.center.y << ',' << bound.center.z << ',' << bound.radius << ']';
        std::cout << ",\"has_colors\":" << (data->HasVertexColors() ? "true" : "false") << ",\"colors\":";
        array(data->vertexColors, [](const Color4& c) { std::cout << '[' << c.r << ',' << c.g << ',' << c.b << ',' << c.a << ']'; });
        std::cout << ",\"uv_sets\":";
        array(data->uvSets, [](const auto& uv) { array(uv, [](const Vector2& v) { std::cout << '[' << v.u << ',' << v.v << ']'; }); });
        std::cout << ",\"consistency_flags\":" << data->consistencyFlags << ",\"additional_data\":";
        ref(data->additionalDataRef.index);
        std::vector<Triangle> triangles;
        data->GetTriangles(triangles);
        std::cout << ",\"triangles\":"; array(triangles, triangle);
        if (auto strips = dynamic_cast<NiTriStripsData*>(data)) {
            std::cout << ",\"strip_lengths\":";
            array(strips->stripsInfo.stripLengths, [](uint16_t n) { std::cout << n; });
            std::cout << ",\"has_points\":" << (strips->stripsInfo.hasPoints ? "true" : "false") << ",\"strip_indices\":";
            array(strips->stripsInfo.points, [](const auto& strip) { array(strip, [](uint16_t n) { std::cout << n; }); });
        } else {
            std::cout << ",\"match_groups\":";
            array(dynamic_cast<NiTriShapeData*>(data)->GetMatchGroups(), [](const MatchGroup& group) {
                array(group.matches, [](uint16_t n) { std::cout << n; });
            });
        }
        std::cout << '}';
    }
    // Compose through nifly's math API, independently of the Rust traversal.
    // A bounded ancestor walk also handles objects outside the footer-root tree.
    std::vector<uint32_t> parent(blocks.size(), UINT32_MAX);
    for (uint32_t id : selected) if (auto node = header.GetBlock<NiNode>(id)) {
        for (auto it = node->childRefs.cbegin(); it != node->childRefs.cend(); ++it) {
            if (it->IsEmpty()) continue;
            if (it->index >= blocks.size() || parent[it->index] != UINT32_MAX)
                throw std::runtime_error("invalid or multiple parent in oracle scene");
            parent[it->index] = id;
        }
    }
    std::cout << "],\"world_transforms\":[";
    first = true;
    for (uint32_t id : selected) {
        auto combined = header.GetBlock<NiAVObject>(id)->transform;
        uint32_t ancestor = parent[id];
        size_t steps = 0;
        while (ancestor != UINT32_MAX) {
            if (++steps > blocks.size()) throw std::runtime_error("cycle in oracle scene");
            combined = header.GetBlock<NiAVObject>(ancestor)->transform.ComposeTransforms(combined);
            ancestor = parent[ancestor];
        }
        if (!first) std::cout << ',';
        first = false;
        std::cout << "{\"block\":" << id << ",\"parent\":"; ref(parent[id]);
        std::cout << ",\"matrix\":[";
        const auto matrix = combined.ToMatrix();
        for (int row = 0; row < 3; ++row) {
            if (row) std::cout << ',';
            std::cout << '[';
            for (int col = 0; col < 4; ++col) { if (col) std::cout << ','; std::cout << matrix[row * 4 + col]; }
            std::cout << ']';
        }
        std::cout << "]}";
    }
    std::cout << ']';
    material_oracle::write(header);
    std::cout << '}';
}
} // namespace scene_oracle
