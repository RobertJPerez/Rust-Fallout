// SPDX-License-Identifier: GPL-3.0-only
// Source graph facts from selected raw factories. No skin or world transforms
// are composed here; ancestry through undecoded objects is left unproven.
#pragma once
#include "Nodes.hpp"
#include <cmath>

static bool selected_node(const std::string& name) { return name == "NiNode" || name == "BSFadeNode"; }

static void preflight_node(const std::string& payload, uint32_t version, size_t& storage) {
    size_t offset = 0;
    auto integer = [&](size_t width) { const auto value = raw_uint(payload, offset, width); offset += width; return value; };
    auto reserve = [&](size_t count, size_t width) {
        if (width && count > storage / width) throw std::runtime_error("node source storage budget exceeded");
        storage -= count * width;
    };
    auto refs = [&]() {
        const auto count = integer(4);
        if (count > 2000000 || count > (payload.size() - offset) / 4)
            throw std::runtime_error("node reference array exceeds block/element budget");
        // Also bound graph/unknown-edge scratch attributable to these refs.
        reserve(count, 2 * sizeof(NiBlockRef<NiObject>) + sizeof(uint32_t) * 2);
        offset += size_t(count) * 4;
    };
    reserve(1, sizeof(NiNode));
    integer(4); refs(); integer(4); integer(version <= 26 ? 2 : 4);
    for (int i = 0; i < 13; ++i) {
        auto word = integer(4); float value; std::memcpy(&value, &word, 4);
        if (!std::isfinite(value)) throw std::runtime_error("nonfinite node source float");
    }
    refs(); integer(4); refs(); refs();
    if (offset != payload.size()) throw std::runtime_error("node source has surplus bytes");
}

struct BindingGraph {
    struct UnknownEdge { uint32_t parent, target; };
    std::vector<uint32_t> parents;
    std::vector<bool> known, reachable;
    std::vector<UnknownEdge> unknown;
    size_t checks = 16000000;
    void charge(size_t count = 1) {
        if (count > checks) throw std::runtime_error("native source graph-check budget exceeded");
        checks -= count;
    }
    BindingGraph(NiHeader& header, size_t count) : parents(count, UINT32_MAX), known(count, false), reachable(count, false) {
        charge(count);
        size_t selected = 0;
        for (uint32_t id = 0; id < count; ++id) { known[id] = header.GetBlock<NiAVObject>(id) != nullptr; selected += known[id]; }
        for (uint32_t id = 0; id < count; ++id) if (auto node = header.GetBlock<NiNode>(id)) {
            for (auto it = node->childRefs.cbegin(); it != node->childRefs.cend(); ++it) {
                charge();
                const auto child = it->index;
                if (child == UINT32_MAX) continue;
                if (child >= count) throw std::runtime_error("node child reference out of range");
                if (known[child]) {
                    if (parents[child] != UINT32_MAX) throw std::runtime_error("repeated or multiple decoded scene parents");
                    parents[child] = id;
                } else {
                    const auto name = header.GetBlockTypeStringById(child);
                    if (name == "NiTriShapeData" || name == "NiTriStripsData") throw std::runtime_error("scene child targets geometry data");
                    unknown.push_back({id, child});
                }
            }
        }
        for (auto root : header.GetRootBlockIds()) {
            charge();
            if (root == UINT32_MAX) continue;
            if (root >= count) throw std::runtime_error("footer root out of range");
            if (known[root]) {
                if (parents[root] != UINT32_MAX) throw std::runtime_error("footer root also has decoded parent");
                reachable[root] = true;
            } else unknown.push_back({UINT32_MAX, root});
        }
        std::vector<uint32_t> queue;
        queue.reserve(selected);
        for (uint32_t id = 0; id < count; ++id) if (known[id] && parents[id] == UINT32_MAX) queue.push_back(id);
        for (size_t position = 0; position < queue.size(); ++position) {
            charge();
            const auto id = queue[position];
            if (auto node = header.GetBlock<NiNode>(id)) {
                for (auto it = node->childRefs.cbegin(); it != node->childRefs.cend(); ++it) {
                    const auto child = it->index;
                    if (child != UINT32_MAX && known[child]) { reachable[child] = reachable[id]; queue.push_back(child); }
                }
            }
        }
        if (queue.size() != selected) throw std::runtime_error("cycle in decoded source graph");
    }
    int contains(NiHeader& header, uint32_t root, uint32_t target, bool bone) {
        if (!header.GetBlock<NiNode>(root) || (bone && !header.GetBlock<NiNode>(target))) return -1;
        for (auto ancestor = target; ancestor != UINT32_MAX; ancestor = parents[ancestor]) {
            charge();
            if (ancestor == root) return 1;
        }
        return 0;
    }
};

static void nullable_bool(std::ostream& out, int value) { if (value < 0) out << "null"; else out << (value ? "true" : "false"); }
template<class T> static void source_refs(std::ostream& out, const NiBlockRefArray<T>& refs) {
    out << '[';
    for (uint32_t i = 0; i < refs.GetSize(); ++i) { if (i) out << ','; ref(out, refs.GetBlockRef(i)); }
    out << ']';
}
static void source_target(std::ostream& out, NiHeader& header, const BindingGraph& graph, uint32_t target) {
    const bool decoded = header.GetBlock<NiNode>(target) != nullptr;
    out << "{\"target\":"; ref(out, target);
    out << ",\"decoded_node\":" << (decoded ? "true" : "false") << ",\"parent\":";
    ref(out, decoded ? graph.parents[target] : UINT32_MAX);
    out << ",\"reachable_from_footer\":";
    nullable_bool(out, decoded ? int(graph.reachable[target]) : -1); out << '}';
}

static void write_bindings(std::ostream& out, NiHeader& header, const std::string& bytes, const std::vector<size_t>& offsets) {
    BindingGraph graph(header, offsets.size());
    out << ",\"bindings\":{\"ancestry_scope\":\"decoded-source-forest\",\"nodes\":[";
    bool first = true;
    for (uint32_t id = 0; id < offsets.size(); ++id) {
        auto node = header.GetBlock<NiNode>(id);
        if (!node) continue;
        if (!first) out << ','; first = false;
        out << "{\"block\":" << id << ",\"block_type\":" << std::quoted(header.GetBlockTypeStringById(id))
            << ",\"offset\":" << offsets[id] << ",\"bytes\":" << header.GetBlockSize(id) << ",\"sha256\":"
            << std::quoted(sha256(bytes.data() + offsets[id], header.GetBlockSize(id))) << ",\"name\":";
        ref(out, node->name.GetIndex()); out << ",\"extra_data\":"; source_refs(out, node->extraDataRefs);
        out << ",\"controller\":"; ref(out, node->controllerRef.index);
        out << ",\"flags\":" << node->flags << ",\"transform\":"; transform(out, node->transform);
        out << ",\"properties\":"; source_refs(out, node->propertyRefs);
        out << ",\"collision\":"; ref(out, node->collisionRef.index);
        out << ",\"children\":"; source_refs(out, node->childRefs);
        out << ",\"effects\":"; source_refs(out, node->effectRefs);
        out << ",\"parent\":"; ref(out, graph.parents[id]);
        out << ",\"reachable_from_footer\":" << (graph.reachable[id] ? "true" : "false") << '}';
    }
    out << "],\"instances\":["; first = true;
    for (uint32_t id = 0; id < offsets.size(); ++id) {
        auto skin = header.GetBlock<NiSkinInstance>(id);
        if (!skin) continue;
        graph.charge();
        if (!first) out << ','; first = false;
        out << "{\"instance\":" << id << ",\"skeleton_root\":";
        source_target(out, header, graph, skin->targetRef.index);
        out << ",\"bones\":[";
        for (uint32_t ordinal = 0; ordinal < skin->boneRefs.GetSize(); ++ordinal) {
            graph.charge();
            if (ordinal) out << ',';
            const auto target = skin->boneRefs.GetBlockRef(ordinal);
            out << "{\"ordinal\":" << ordinal << ",\"node\":"; source_target(out, header, graph, target);
            out << ",\"decoded_root_contains\":";
            nullable_bool(out, graph.contains(header, skin->targetRef.index, target, true)); out << '}';
        }
        out << "],\"owners\":["; bool first_owner = true;
        for (uint32_t geometry_id = 0; geometry_id < offsets.size(); ++geometry_id) {
            graph.charge();
            auto geometry = header.GetBlock<NiGeometry>(geometry_id);
            if (!geometry || geometry->SkinInstanceRef()->index != id) continue;
            if (!first_owner) out << ','; first_owner = false;
            out << "{\"geometry\":" << geometry_id << ",\"parent\":"; ref(out, graph.parents[geometry_id]);
            out << ",\"reachable_from_footer\":" << (graph.reachable[geometry_id] ? "true" : "false")
                << ",\"decoded_root_contains\":";
            nullable_bool(out, graph.contains(header, skin->targetRef.index, geometry_id, false)); out << '}';
        }
        out << "]}";
    }
    out << "],\"footer_roots\": [";
    const auto& roots = header.GetRootBlockIds();
    for (size_t i = 0; i < roots.size(); ++i) { if (i) out << ','; ref(out, roots[i]); }
    out << "],\"unsupported_scene_edges\":[";
    for (size_t i = 0; i < graph.unknown.size(); ++i) {
        if (i) out << ',';
        const auto& edge = graph.unknown[i];
        out << "{\"parent\":"; ref(out, edge.parent);
        out << ",\"target\":" << edge.target << ",\"block_type\":" << std::quoted(header.GetBlockTypeStringById(edge.target)) << '}';
    }
    out << "]}";
}
