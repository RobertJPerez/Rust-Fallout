// Original offline leveled-list projection. Original headers, source namespaces and
// compressed bodies are read independently; no live inventory is initialized.
#include "../oracle-common/record_source.hpp"
#include "../oracle-common/zlib_source.hpp"
using namespace fallout_records;

static std::string kind_bytes(const Bytes &bytes, size_t offset = 0) {
    std::vector<std::string> parts;
    for (size_t n = 0; n < 4; ++n)
        parts.push_back(std::to_string(integer(bytes, offset + n, 1)));
    return array(parts);
}
static std::string signed_word(uint32_t word) {
    return std::to_string(word & 0x80000000u ? static_cast<int64_t>(word) - 0x100000000LL
                                             : static_cast<int64_t>(word));
}
struct Binding {
    std::string json, status;
    std::optional<std::string> kind;
    std::optional<Key> key;
};
struct Counts {
    std::map<std::string, uint64_t> numbers{
        {"records", 0},         {"deleted_records", 0}, {"decoded_bytes", 0},  {"fields", 0},
        {"entries", 0},         {"extra_fields", 0},    {"absent_counts", 0},  {"zero_counts", 0},
        {"high_bit_counts", 0}, {"high_bit_levels", 0}, {"source_findings", 0}};
    std::map<std::string, uint64_t> kinds, bindings;
    static std::string map_json(const std::map<std::string, uint64_t> &values) {
        Object result;
        for (const auto &value : values)
            result[value.first] = std::to_string(value.second);
        return object(result);
    }
    std::string json() const {
        Object result;
        for (const auto &value : numbers)
            result[value.first] = std::to_string(value.second);
        result["record_kinds"] = map_json(kinds);
        result["binding_statuses"] = map_json(bindings);
        return object(result);
    }
};
static Binding bind(const HeaderIndex &index, size_t source_index, uint32_t raw, Counts &counts) {
    const auto &source = index.plugins[source_index];
    const auto key = resolve(source.name, source.masters, raw);
    const auto winner = key ? index.winners.find(*key) : index.winners.end();
    std::string status = "null", target = "null";
    std::optional<std::string> kind;
    if (key) {
        if (winner == index.winners.end())
            status = "missing";
        else {
            const auto &entry = winner->second;
            const auto flags = static_cast<uint32_t>(integer(entry.header, 8, 4));
            status = flags & 0x20 ? "deleted" : "defined";
            kind = std::string(entry.header.begin(), entry.header.begin() + 4);
            target = object({{"kind", kind_bytes(entry.header)},
                             {"source_plugin", quote(index.plugins[entry.source].name)},
                             {"record_file_offset", std::to_string(entry.offset)},
                             {"record_flags", std::to_string(flags)}});
        }
    }
    ++counts.bindings[status];
    return {object({{"raw_form", std::to_string(raw)},
                    {"key", key ? key->json() : "null"},
                    {"status", quote(status)},
                    {"target", target}}),
            status, kind, key};
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
static std::string allowed(const std::string &source, const std::optional<std::string> &target,
                           const std::string &role = "leveled-entry") {
    if (!target)
        return "null";
    std::set<std::string> kinds;
    if (role == "actor-template")
        kinds = source == "NPC_" ? std::set<std::string>{"NPC_", "LVLN"}
                                 : std::set<std::string>{"CREA", "LVLC"};
    else if (role == "base-item")
        kinds = {"ARMO", "AMMO", "MISC", "WEAP", "BOOK", "LVLI", "KEYM",
                 "ALCH", "NOTE", "IMOD", "CMNY", "CCRD", "LIGH", "CHIP"};
    else if (source == "LVLC")
        kinds = {"CREA", "LVLC"};
    else if (source == "LVLN")
        kinds = {"NPC_", "LVLN"};
    else
        kinds = {"ALCH", "AMMO", "ARMO", "BOOK", "CCRD", "CHIP", "CMNY",
                 "IMOD", "KEYM", "LVLI", "MISC", "NOTE", "WEAP"};
    return kinds.count(*target) ? "true" : "false";
}
struct ListEntry {
    size_t lvlo;
    std::vector<std::string> extras;
};
struct Document {
    std::vector<std::string> fields, findings;
    std::vector<ListEntry> entries;
};
static Document decode(const HeaderIndex &index, const Entry &entry, const Bytes &body,
                       const std::string &source_kind, Counts &counts) {
    Document result;
    std::optional<size_t> pending;
    size_t chances = 0, flags = 0, globals = 0;
    fields(body, [&](const std::string &kind, size_t start, const Bytes &data) {
        if (counts.numbers["fields"] >= 2000000)
            throw std::runtime_error("list field budget");
        const auto exact = [&](size_t n) {
            if (data.size() != n)
                throw std::runtime_error("list known field shape");
        };
        const auto finding = [&](const std::string &code) {
            result.findings.push_back(
                object({{"field_decoded_offset", std::to_string(start)}, {"code", quote(code)}}));
            ++counts.numbers["source_findings"];
        };
        std::string value = object({{"kind", quote("opaque")}});
        if (kind == "LVLO") {
            if (data.size() != 8 && data.size() != 10 && data.size() != 12)
                throw std::runtime_error("list entry shape");
            if (counts.numbers["entries"] >= 1000000)
                throw std::runtime_error("list entry budget");
            const auto level = integer(data, 0, 2);
            const auto raw = static_cast<uint32_t>(integer(data, 4, 4));
            const auto item = bind(index, entry.source, raw, counts);
            const auto count = data.size() >= 10 ? std::to_string(integer(data, 8, 2)) : "null";
            value = object({{"kind", quote("entry")},
                            {"level_bits", std::to_string(level)},
                            {"level_padding", std::to_string(integer(data, 2, 2))},
                            {"item", item.json},
                            {"count_bits", count},
                            {"count_padding",
                             data.size() == 12 ? std::to_string(integer(data, 10, 2)) : "null"},
                            {"schema_kind_allowed", allowed(source_kind, item.kind)}});
            ++counts.numbers["entries"];
            counts.numbers["high_bit_levels"] += bool(level & 0x8000);
            if (data.size() == 8)
                ++counts.numbers["absent_counts"];
            else {
                const auto n = integer(data, 8, 2);
                counts.numbers["zero_counts"] += !n;
                counts.numbers["high_bit_counts"] += bool(n & 0x8000);
            }
            pending = result.entries.size();
            result.entries.push_back({result.fields.size(), {}});
        } else if (kind == "COED") {
            exact(12);
            ++counts.numbers["extra_fields"];
            if (pending) {
                auto &item = result.entries[*pending];
                if (!item.extras.empty())
                    finding("multiple_entry_extra_fields");
                item.extras.push_back(std::to_string(result.fields.size()));
            } else
                finding("orphan_entry_extra_field");
            const auto owner =
                bind(index, entry.source, static_cast<uint32_t>(integer(data, 0, 4)), counts);
            const auto word = static_cast<uint32_t>(integer(data, 4, 4));
            std::string extra;
            if (owner.status == "null")
                extra = object({{"kind", quote("unused")}, {"raw_word", std::to_string(word)}});
            else if (owner.status == "defined" && owner.kind == "NPC_")
                extra = object({{"kind", quote("global")},
                                {"binding", bind(index, entry.source, word, counts).json}});
            else if (owner.status == "defined" && owner.kind == "FACT")
                extra = object({{"kind", quote("required_rank")},
                                {"raw_word", std::to_string(word)},
                                {"value", signed_word(word)}});
            else
                extra = object(
                    {{"kind", quote("unresolved_owner")}, {"raw_word", std::to_string(word)}});
            value = object({{"kind", quote("extra")},
                            {"owner", owner.json},
                            {"union_word", extra},
                            {"condition_bits", std::to_string(integer(data, 8, 4))}});
        } else if (kind == "LVLD") {
            exact(1);
            pending.reset();
            if (++chances > 1)
                finding("multiple_chance_none_fields");
            value = object({{"kind", quote("chance_none")}, {"raw", std::to_string(data[0])}});
        } else if (kind == "LVLF") {
            exact(1);
            pending.reset();
            if (++flags > 1)
                finding("multiple_list_flag_fields");
            value = object(
                {{"kind", quote("flags")},
                 {"raw", std::to_string(data[0])},
                 {"all_lower_levels", data[0] & 1 ? "true" : "false"},
                 {"each_count", data[0] & 2 ? "true" : "false"},
                 {"use_all", source_kind == "LVLI" ? (data[0] & 4 ? "true" : "false") : "null"}});
        } else if (kind == "LVLG" && source_kind == "LVLI") {
            exact(4);
            pending.reset();
            if (++globals > 1)
                finding("multiple_chance_global_fields");
            value = object({{"kind", quote("global")},
                            {"global", bind(index, entry.source,
                                            static_cast<uint32_t>(integer(data, 0, 4)), counts)
                                           .json}});
        } else
            pending.reset();
        result.fields.push_back(object({{"kind", kind_bytes(body, start)},
                                        {"decoded_offset", std::to_string(start)},
                                        {"bytes", std::to_string(data.size())},
                                        {"sha256", quote(fallout_tables::hash(data))},
                                        {"value", value}}));
        ++counts.numbers["fields"];
    });
    return result;
}

// Tarjan's algorithm uses explicit DFS frames, independently of the Rust planner.
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
static std::string graph(const HeaderIndex &index) {
    std::vector<Key> keys;
    std::vector<std::string> nodes, edges;
    std::map<Key, size_t> positions;
    for (const auto &winner : index.winners) {
        const auto &entry = winner.second;
        const std::string kind(entry.header.begin(), entry.header.begin() + 4);
        if (!std::set<std::string>{"CONT", "NPC_", "CREA", "LVLI", "LVLC", "LVLN"}.count(kind))
            continue;
        if (keys.size() >= 131072)
            throw std::runtime_error("list graph node budget");
        positions[winner.first] = keys.size();
        keys.push_back(winner.first);
        nodes.push_back(
            object({{"key", winner.first.json()},
                    {"kind", kind_bytes(entry.header)},
                    {"deleted", integer(entry.header, 8, 4) & 0x20 ? "true" : "false"}}));
    }
    std::vector<std::vector<size_t>> children(keys.size());
    uint64_t bytes = 0, internal = 0, terminal = 0, unresolved = 0, mismatches = 0, field_count = 0;
    Counts ignored;
    for (size_t n = 0; n < keys.size(); ++n) {
        const auto &entry = index.winners.at(keys[n]);
        if (integer(entry.header, 8, 4) & 0x20)
            continue;
        const std::string source_kind(entry.header.begin(), entry.header.begin() + 4);
        const auto payload = body(index, entry, bytes, 384ULL * 1024 * 1024);
        fields(payload, [&](const std::string &kind, size_t offset, const Bytes &data) {
            if (++field_count > 4000000)
                throw std::runtime_error("list graph field budget");
            std::string role;
            uint32_t raw = 0;
            if (kind == "CNTO" &&
                (source_kind == "CONT" || source_kind == "NPC_" || source_kind == "CREA")) {
                if (data.size() != 8)
                    throw std::runtime_error("graph CNTO shape");
                role = "base-item";
                raw = static_cast<uint32_t>(integer(data, 0, 4));
            } else if (kind == "TPLT" && (source_kind == "NPC_" || source_kind == "CREA")) {
                if (data.size() != 4)
                    throw std::runtime_error("graph TPLT shape");
                role = "actor-template";
                raw = static_cast<uint32_t>(integer(data, 0, 4));
            } else if (kind == "LVLO" &&
                       (source_kind == "LVLI" || source_kind == "LVLC" || source_kind == "LVLN")) {
                if (data.size() != 8 && data.size() != 10 && data.size() != 12)
                    throw std::runtime_error("graph LVLO shape");
                role = "leveled-entry";
                raw = static_cast<uint32_t>(integer(data, 4, 4));
            } else
                return;
            if (edges.size() >= 2000000)
                throw std::runtime_error("list graph edge budget");
            const auto target = bind(index, entry.source, raw, ignored);
            const auto domain = allowed(source_kind, target.kind, role);
            mismatches += domain == "false";
            edges.push_back(object({{"source", keys[n].json()},
                                    {"field_decoded_offset", std::to_string(offset)},
                                    {"role", quote(role)},
                                    {"target", target.key ? target.key->json() : "null"},
                                    {"status", quote(target.status)},
                                    {"schema_kind_allowed", domain}}));
            if (target.status != "defined") {
                ++unresolved;
                return;
            }
            const auto position = target.key ? positions.find(*target.key) : positions.end();
            if (position == positions.end())
                ++terminal;
            else {
                ++internal;
                children[n].push_back(position->second);
            }
        });
    }
    const auto components = cycles(children);
    std::vector<std::string> groups;
    size_t cyclic_nodes = 0;
    for (const auto &component : components) {
        std::vector<std::string> members;
        for (auto n : component)
            members.push_back(keys[n].json());
        groups.push_back(array(members));
        cyclic_nodes += component.size();
    }
    return object({{"nodes", array(nodes)},
                   {"edges", array(edges)},
                   {"cycles", array(groups)},
                   {"counts", object({{"nodes", std::to_string(nodes.size())},
                                      {"edges", std::to_string(edges.size())},
                                      {"internal_edges", std::to_string(internal)},
                                      {"terminal_edges", std::to_string(terminal)},
                                      {"unresolved_edges", std::to_string(unresolved)},
                                      {"schema_mismatches", std::to_string(mismatches)},
                                      {"cyclic_components", std::to_string(groups.size())},
                                      {"cyclic_nodes", std::to_string(cyclic_nodes)}})}});
}
int wmain(int argc, wchar_t **argv) {
    try {
        if (argc != 3)
            throw std::runtime_error("usage: leveled-oracle Data_directory order_bundle");
        auto index = scan(argv[1], argv[2]);
        Counts counts;
        std::vector<std::string> definitions;
        for (const auto &winner : index.winners) {
            const auto &entry = winner.second;
            const auto &source = index.plugins[entry.source];
            const std::string kind(entry.header.begin(), entry.header.begin() + 4);
            if (kind != "LVLI" && kind != "LVLC" && kind != "LVLN")
                continue;
            if (++counts.numbers["records"] > 65536)
                throw std::runtime_error("list record budget");
            ++counts.kinds[kind];
            const auto flags = static_cast<uint32_t>(integer(entry.header, 8, 4));
            const bool deleted = flags & 0x20;
            Document document;
            std::string hash = "null";
            if (deleted)
                ++counts.numbers["deleted_records"];
            else {
                const auto decoded =
                    body(index, entry, counts.numbers["decoded_bytes"], 128ULL * 1024 * 1024);
                hash = quote(fallout_tables::hash(decoded));
                document = decode(index, entry, decoded, kind, counts);
            }
            std::vector<std::string> entries;
            for (const auto &e : document.entries)
                entries.push_back(object(
                    {{"lvlo_field", std::to_string(e.lvlo)}, {"coed_fields", array(e.extras)}}));
            definitions.push_back(
                object({{"key", winner.first.json()},
                        {"kind", kind_bytes(entry.header)},
                        {"source", object({{"plugin", quote(source.name)},
                                           {"sha256", quote(source.source->sha256)},
                                           {"record_file_offset", std::to_string(entry.offset)},
                                           {"record_flags", std::to_string(flags)},
                                           {"decoded_record_sha256", hash}})},
                        {"deleted", deleted ? "true" : "false"},
                        {"fields", array(document.fields)},
                        {"entries", array(entries)},
                        {"findings", array(document.findings)}}));
        }
        std::cout << object({{"schema_version", "1"},
                             {"profile", quote("nv-original")},
                             {"sources", index.sources_json()},
                             {"metadata", index.metadata_json()},
                             {"counts", counts.json()},
                             {"definitions", array(definitions)},
                             {"dependency_graph", graph(index)}})
                  << '\n';
        return 0;
    } catch (const std::exception &e) {
        std::cerr << "leveled-oracle: " << e.what() << '\n';
        return 1;
    }
}
