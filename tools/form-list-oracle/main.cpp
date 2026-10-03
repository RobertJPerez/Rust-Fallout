// Independent offline FLST source projection. This reader opens original plugins,
// resolves their own master namespaces and retains physical member occurrences.
#include "../oracle-common/record_source.hpp"
#include "../oracle-common/zlib_source.hpp"
using namespace fallout_records;
static std::string kind_bytes(const Bytes &bytes, size_t offset = 0) {
    std::vector<std::string> parts;
    for (size_t n = 0; n < 4; ++n)
        parts.push_back(std::to_string(integer(bytes, offset + n, 1)));
    return array(parts);
}
static Bytes body(const HeaderIndex &index, const Entry &entry, uint64_t &total, uint64_t maximum) {
    const auto size = static_cast<size_t>(integer(entry.header, 4, 4));
    if (size > 64 * 1024 * 1024)
        throw std::runtime_error("list stored byte budget");
    auto result = index.plugins[entry.source].source->read(entry.offset + 24, size);
    if (integer(entry.header, 8, 4) & 0x40000) {
        const auto expected = static_cast<size_t>(integer(result, 0, 4));
        if (expected > 64 * 1024 * 1024)
            throw std::runtime_error("list decoded record budget");
        const Bytes frame(result.begin() + 4, result.end());
        auto decoded = fallout_zlib::decode(frame, expected);
        if (decoded.stored_adler != decoded.calculated_adler)
            throw std::runtime_error("list source checksum");
        result = std::move(decoded.payload);
    }
    if (result.size() > maximum - total)
        throw std::runtime_error("list decoded cohort budget");
    total += result.size();
    return result;
}
template <class Visitor> static void fields(const Bytes &body, Visitor visitor) {
    std::optional<size_t> extended;
    for (size_t cursor = 0; cursor < body.size();) {
        if (body.size() - cursor < 6)
            throw std::runtime_error("list field header");
        const auto start = cursor;
        const std::string kind(body.begin() + cursor, body.begin() + cursor + 4);
        const auto small = static_cast<size_t>(integer(body, cursor + 4, 2));
        cursor += 6;
        if (kind == "XXXX") {
            if (small != 4 || extended)
                throw std::runtime_error("list extended prefix");
            extended = static_cast<size_t>(integer(body, cursor, 4));
            cursor += 4;
            continue;
        }
        const auto size = extended.value_or(small);
        extended.reset();
        if (size > body.size() - cursor)
            throw std::runtime_error("list field extent");
        const Bytes payload(body.begin() + cursor, body.begin() + cursor + size);
        cursor += size;
        visitor(kind, start, payload);
    }
    if (extended)
        throw std::runtime_error("orphan list extended prefix");
}
static std::vector<std::vector<size_t>> cycles(const std::vector<std::vector<size_t>> &children) {
    const auto unset = static_cast<size_t>(-1);
    size_t next = 0;
    std::vector<size_t> number(children.size(), unset), low(children.size()), active_stack;
    std::vector<bool> active(children.size());
    std::vector<std::vector<size_t>> result;
    struct Frame {
        size_t node, child;
    };
    for (size_t root = 0; root < children.size(); ++root) {
        if (number[root] != unset)
            continue;
        number[root] = low[root] = next++;
        active[root] = true;
        active_stack.push_back(root);
        std::vector<Frame> frames{{root, 0}};
        while (!frames.empty()) {
            auto &frame = frames.back();
            const auto node = frame.node;
            if (frame.child < children[node].size()) {
                const auto to = children[node][frame.child++];
                if (number[to] == unset) {
                    number[to] = low[to] = next++;
                    active[to] = true;
                    active_stack.push_back(to);
                    frames.push_back({to, 0});
                } else if (active[to])
                    low[node] = std::min(low[node], number[to]);
            } else {
                if (low[node] == number[node]) {
                    std::vector<size_t> component;
                    size_t member;
                    do {
                        member = active_stack.back();
                        active_stack.pop_back();
                        active[member] = false;
                        component.push_back(member);
                    } while (member != node);
                    std::sort(component.begin(), component.end());
                    if (component.size() > 1 ||
                        std::find(children[node].begin(), children[node].end(), node) !=
                            children[node].end())
                        result.push_back(std::move(component));
                }
                frames.pop_back();
                if (!frames.empty())
                    low[frames.back().node] = std::min(low[frames.back().node], low[node]);
            }
        }
    }
    std::sort(result.begin(), result.end());
    return result;
}

struct Edge {
    Key source;
    size_t offset;
    std::optional<Key> target;
    std::string status, is_list;
    std::string json() const {
        return object({{"source", source.json()},
                       {"field_decoded_offset", std::to_string(offset)},
                       {"target", target ? target->json() : "null"},
                       {"status", quote(status)},
                       {"target_is_list", is_list}});
    }
};
int wmain(int argc, wchar_t **argv) {
    try {
        if (argc != 3)
            throw std::runtime_error("usage: form-list-oracle Data_directory order_bundle");
        auto index = scan(argv[1], argv[2]);
        std::vector<Key> keys;
        std::vector<std::string> nodes, definitions;
        std::map<Key, size_t> positions;
        for (const auto &winner : index.winners) {
            const auto &entry = winner.second;
            if (std::string(entry.header.begin(), entry.header.begin() + 4) != "FLST")
                continue;
            if (keys.size() >= 65536)
                throw std::runtime_error("form list record budget");
            positions[winner.first] = keys.size();
            keys.push_back(winner.first);
            nodes.push_back(
                object({{"key", winner.first.json()},
                        {"deleted", integer(entry.header, 8, 4) & 0x20 ? "true" : "false"}}));
        }
        uint64_t decoded_bytes = 0, field_count = 0, entry_count = 0, deleted = 0;
        std::map<std::string, uint64_t> statuses;
        std::vector<Edge> edges;
        for (const auto &key : keys) {
            const auto &entry = index.winners.at(key);
            const auto &source = index.plugins[entry.source];
            const auto flags = static_cast<uint32_t>(integer(entry.header, 8, 4));
            std::vector<std::string> projected, members;
            std::string payload_hash = "null";
            if (flags & 0x20)
                ++deleted;
            else {
                const auto payload = body(index, entry, decoded_bytes, 128ULL * 1024 * 1024);
                payload_hash = quote(fallout_tables::hash(payload));
                fields(payload, [&](const std::string &kind, size_t offset, const Bytes &bytes) {
                    if (field_count++ >= 1000000)
                        throw std::runtime_error("form list field budget");
                    std::string value = object({{"kind", quote("opaque")}});
                    if (kind == "LNAM") {
                        if (bytes.size() != 4)
                            throw std::runtime_error("FLST LNAM extent");
                        if (entry_count++ >= 1000000)
                            throw std::runtime_error("form list member budget");
                        const auto raw = static_cast<uint32_t>(integer(bytes, 0, 4));
                        const auto target = resolve(source.name, source.masters, raw);
                        std::string status = "null", binding_target = "null", is_list = "null";
                        if (target) {
                            const auto found = index.winners.find(*target);
                            if (found == index.winners.end())
                                status = "missing";
                            else {
                                const auto &to = found->second;
                                const auto target_flags =
                                    static_cast<uint32_t>(integer(to.header, 8, 4));
                                status = target_flags & 0x20 ? "deleted" : "defined";
                                is_list =
                                    std::string(to.header.begin(), to.header.begin() + 4) == "FLST"
                                        ? "true"
                                        : "false";
                                binding_target =
                                    object({{"kind", kind_bytes(to.header)},
                                            {"source_plugin", quote(index.plugins[to.source].name)},
                                            {"record_file_offset", std::to_string(to.offset)},
                                            {"record_flags", std::to_string(target_flags)}});
                            }
                        }
                        ++statuses[status];
                        members.push_back(std::to_string(projected.size()));
                        edges.push_back({key, offset, target, status, is_list});
                        value = object({{"kind", quote("member")},
                                        {"form", object({{"raw_form", std::to_string(raw)},
                                                         {"key", target ? target->json() : "null"},
                                                         {"status", quote(status)},
                                                         {"target", binding_target}})}});
                    }
                    const Bytes tag(kind.begin(), kind.end());
                    projected.push_back(object({{"kind", kind_bytes(tag)},
                                                {"decoded_offset", std::to_string(offset)},
                                                {"bytes", std::to_string(bytes.size())},
                                                {"sha256", quote(fallout_tables::hash(bytes))},
                                                {"value", value}}));
                });
            }
            definitions.push_back(
                object({{"key", key.json()},
                        {"source", object({{"plugin", quote(source.name)},
                                           {"sha256", quote(source.source->sha256)},
                                           {"record_file_offset", std::to_string(entry.offset)},
                                           {"record_flags", std::to_string(flags)},
                                           {"decoded_record_sha256", payload_hash}})},
                        {"deleted", flags & 0x20 ? "true" : "false"},
                        {"fields", array(projected)},
                        {"entries", array(members)}}));
        }
        std::vector<std::vector<size_t>> children(keys.size());
        std::vector<std::string> edge_json, cycle_json;
        uint64_t internal = 0, terminal = 0, unresolved = 0, cyclic_nodes = 0;
        for (const auto &edge : edges) {
            edge_json.push_back(edge.json());
            if (edge.status != "defined") {
                ++unresolved;
                continue;
            }
            const auto to = positions.find(*edge.target);
            if (to == positions.end())
                ++terminal;
            else {
                ++internal;
                children[positions.at(edge.source)].push_back(to->second);
            }
        }
        for (const auto &component : cycles(children)) {
            std::vector<std::string> members;
            for (const auto node : component)
                members.push_back(keys[node].json());
            cyclic_nodes += component.size();
            cycle_json.push_back(array(members));
        }
        Object status_json;
        for (const auto &status : statuses)
            status_json[status.first] = std::to_string(status.second);
        std::cout
            << object({{"schema_version", "1"},
                       {"profile", quote("nv-original")},
                       {"sources", index.sources_json()},
                       {"metadata", index.metadata_json()},
                       {"counts", object({{"records", std::to_string(keys.size())},
                                          {"deleted_records", std::to_string(deleted)},
                                          {"decoded_bytes", std::to_string(decoded_bytes)},
                                          {"fields", std::to_string(field_count)},
                                          {"entries", std::to_string(entry_count)},
                                          {"binding_statuses", object(status_json)}})},
                       {"definitions", array(definitions)},
                       {"dependency_graph",
                        object({{"nodes", array(nodes)},
                                {"edges", array(edge_json)},
                                {"cycles", array(cycle_json)},
                                {"counts",
                                 object({{"nodes", std::to_string(keys.size())},
                                         {"edges", std::to_string(edges.size())},
                                         {"internal_edges", std::to_string(internal)},
                                         {"terminal_edges", std::to_string(terminal)},
                                         {"unresolved_edges", std::to_string(unresolved)},
                                         {"cyclic_components", std::to_string(cycle_json.size())},
                                         {"cyclic_nodes", std::to_string(cyclic_nodes)}})}})}})
            << '\n';
        return 0;
    } catch (const std::exception &error) {
        std::cerr << "form-list-oracle: " << error.what() << '\n';
        return 1;
    }
}
