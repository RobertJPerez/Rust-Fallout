// Original offline loaded-script comparison. Original executable code is never
// loaded. Header identities come directly from locked source plugins; script
// fields come from the local decoded bundle, whose extraction is a separate gate.
#include "../oracle-common/record_source.hpp"
using namespace fallout_records;

static bool one_of(const std::string& value, std::initializer_list<const char*> values) {
    return std::any_of(values.begin(), values.end(), [&](const auto item) { return value == item; });
}
static Bytes take(const Bytes& bytes, size_t& cursor, size_t length) {
    if (cursor > bytes.size() || length > bytes.size() - cursor) throw std::runtime_error("bundle extent");
    Bytes result(bytes.begin() + cursor, bytes.begin() + cursor + length); cursor += length; return result;
}
static std::string optional_json(std::optional<size_t> value) { return value ? std::to_string(*value) : "null"; }
static std::vector<fallout_tables::Field> fields(const Bytes& payload) {
    std::vector<fallout_tables::Field> result; std::optional<size_t> extended;
    for (size_t cursor = 0; cursor < payload.size();) {
        const size_t offset = cursor;
        auto name = take(payload, cursor, 4); const std::string kind(name.begin(), name.end());
        const auto short_size = static_cast<size_t>(integer(payload, cursor, 2)); cursor += 2;
        if (kind == "XXXX") {
            if (extended || short_size != 4) throw std::runtime_error("XXXX shape");
            extended = static_cast<size_t>(integer(payload, cursor, 4)); cursor += 4; continue;
        }
        if (result.size() >= 1048576) throw std::runtime_error("field budget");
        result.push_back({kind, offset, take(payload, cursor, extended.value_or(short_size))}); extended.reset();
    }
    if (extended) throw std::runtime_error("orphan XXXX"); return result;
}
struct Owner {
    std::string kind = "unverified_embedded"; std::optional<size_t> section, stage; bool verified = false;
    std::string json() const { return object({{"kind", quote(kind)}, {"section_marker", optional_json(section)},
        {"stage_marker", optional_json(stage)}, {"schema_ownership_verified", verified ? "true" : "false"}}); }
};
struct Owners { std::map<size_t, Owner> units; uint64_t findings = 0; };
static Owners owners(const std::string& record, const std::vector<fallout_tables::Field>& source, size_t unit_count) {
    Owners result;
    if (record == "SCPT") {
        for (const auto& field : source) if (field.kind == "SCHR") result.units[field.offset] = {"standalone", {}, {}, unit_count == 1};
        result.findings = unit_count != 1; return result;
    }
    if (record != "INFO" && record != "QUST") return result;
    std::optional<size_t> stage, entry, objective, target, response, script;
    bool entry_parent = false, end_script = false; size_t begin_count = 0, end_count = 0;
    for (const auto& field : source) {
        const auto& kind = field.kind;
        if (record == "QUST") {
            if (kind == "INDX") { stage = field.offset; entry.reset(); objective.reset(); target.reset(); }
            else if (kind == "QSDT") { entry = field.offset; entry_parent = stage.has_value(); result.findings += !entry_parent; }
            else if (kind == "QOBJ") { objective = field.offset; stage.reset(); entry.reset(); target.reset(); }
            else if (kind == "QSTA") { target = field.offset; result.findings += !objective; }
            else if (one_of(kind, {"CNAM", "NAM0"}) || fallout_tables::selected(kind)) {
                result.findings += !entry;
                if (kind == "SCHR" && entry) result.units[field.offset] = {"quest_log_entry", entry, stage, entry_parent};
            } else if (kind == "NNAM") result.findings += !objective;
            else if (kind == "CTDA") result.findings += (target || objective) ? !target : (entry || stage) ? !entry : false;
            else if (kind == "DATA") result.findings += field.data.size() == 2;
            else if (!one_of(kind, {"EDID", "SCRI", "FULL", "ICON"})) ++result.findings;
        } else {
            if (kind == "TRDT") { script.reset(); response = field.offset; }
            else if (one_of(kind, {"NAM1", "NAM2", "NAM3", "SNAM", "LNAM"})) result.findings += !response;
            else if (kind == "NEXT") { response.reset(); script.reset(); end_script = true; }
            else if (kind == "SCHR") {
                response.reset(); script = field.offset; auto& count = end_script ? end_count : begin_count;
                result.findings += count++ != 0;
                result.units[field.offset] = {end_script ? "dialogue_end" : "dialogue_begin", field.offset, {}, true};
            } else if (fallout_tables::selected(kind)) result.findings += !script;
            else if (one_of(kind, {"CTDA", "DATA", "QSTI", "TPIC", "PNAM", "NAME", "TCLT", "TCLF", "TCFU", "SNDD", "RNAM", "ANAM", "KNAM", "DNAM"})) { response.reset(); script.reset(); }
            else ++result.findings;
        }
    }
    return result;
}
static void version_text(Bytes& out, const std::string& value) {
    append(out, value.size(), 4); out.insert(out.end(), value.begin(), value.end());
}
static std::string version_hash(const Key& key, size_t marker, const Plugin& plugin, const Entry& entry,
    const std::string& body_hash, const std::string& metadata_hash, const std::optional<Bytes>& compiled) {
    Bytes bytes{'F','R','S','C','R','V','0','1'};
    version_text(bytes, "nv-original"); version_text(bytes, key.origin); append(bytes, key.local, 4); append(bytes, marker, 4);
    version_text(bytes, plugin.name); version_text(bytes, plugin.source->sha256);
    append(bytes, entry.offset, 8); append(bytes, integer(entry.header, 8, 4), 4);
    version_text(bytes, body_hash); version_text(bytes, metadata_hash); append(bytes, compiled.has_value(), 1);
    if (compiled) { version_text(bytes, fallout_tables::hash(*compiled)); append(bytes, compiled->size(), 8); }
    return fallout_tables::hash(bytes);
}
struct Counts {
    std::map<std::string, uint64_t> numbers, kinds, owners, statuses;
    Counts() { for (const auto key : {"records_retained", "payload_bytes_retained", "scripts", "compiled_bodies", "compiled_bytes", "variables",
        "duplicate_variable_indices", "references", "scripts_with_issues", "source_ownership_findings"}) numbers[key] = 0; }
    std::string json() const {
        auto map_json = [](const auto& values) { Object result; for (const auto& value : values) result[value.first] = std::to_string(value.second); return object(result); };
        Object result; for (const auto& value : numbers) result[value.first] = std::to_string(value.second);
        result["record_kinds"] = map_json(kinds); result["owners"] = map_json(owners); result["reference_statuses"] = map_json(statuses); return object(result);
    }
};
static std::string unit_json(const HeaderIndex& index, const Key& key, const Entry& record, const std::string& body_hash,
    const std::vector<fallout_tables::Field>& fields, const Owner& owner, Counts& counts) {
    const auto& plugin = index.plugins[record.source]; const size_t marker = fields.front().offset;
    Bytes metadata; std::optional<Bytes> compiled, declaration; std::optional<size_t> declaration_offset;
    std::map<uint32_t, size_t> first_variable; std::vector<std::string> variables, references, issues;
    std::vector<const fallout_tables::Field*> reference_fields; bool source_text = false;
    for (const auto& field : fields) {
        metadata.insert(metadata.end(), field.kind.begin(), field.kind.end()); append(metadata, field.offset, 4); append(metadata, field.data.size(), 4);
        metadata.insert(metadata.end(), field.data.begin(), field.data.end());
        if (declaration && field.kind != "SCVR") throw std::runtime_error("unnamed declaration");
        if (field.kind == "SCDA") { if (compiled) throw std::runtime_error("duplicate compiled body"); compiled = field.data; }
        else if (field.kind == "SCTX") { if (source_text) throw std::runtime_error("duplicate source text"); source_text = true; }
        else if (field.kind == "SLSD") { if (field.data.size() != 24 || variables.size() >= 65536) throw std::runtime_error("declaration extent/budget"); declaration = field.data; declaration_offset = field.offset; }
        else if (field.kind == "SCVR") {
            if (!declaration || field.data.empty() || field.data.back() || std::find(field.data.begin(), field.data.end() - 1, 0) != field.data.end() - 1) throw std::runtime_error("variable name pairing/termination");
            const auto variable = static_cast<uint32_t>(integer(*declaration, 0, 4));
            if (!first_variable.emplace(variable, *declaration_offset).second) ++counts.numbers["duplicate_variable_indices"];
            variables.push_back(object({{"index", std::to_string(variable)}, {"type_byte", std::to_string((*declaration)[16])},
                {"decoded_offset", std::to_string(*declaration_offset)}, {"name_bytes", std::to_string(field.data.size())}, {"name_sha256", quote(fallout_tables::hash(field.data))}}));
            declaration.reset();
        } else if (field.kind == "SCRO" || field.kind == "SCRV") {
            if (field.data.size() != 4 || reference_fields.size() >= 65536) throw std::runtime_error("reference extent/budget"); reference_fields.push_back(&field);
        }
    }
    if (declaration) throw std::runtime_error("unnamed final declaration");
    if (compiled) {
        if (compiled->size() > 4 * 1024 * 1024) throw std::runtime_error("compiled byte budget");
        size_t instruction_count = 0;
        for (size_t cursor = 0; cursor < compiled->size();) {
            if (++instruction_count > 262144) throw std::runtime_error("instruction budget");
            const bool reference_call = integer(*compiled, cursor, 2) == 0x1c;
            const size_t header_size = reference_call ? 8 : 4;
            const auto opcode = integer(*compiled, cursor + (reference_call ? 4 : 0), 2);
            const auto length = static_cast<size_t>(integer(*compiled, cursor + header_size - 2, 2));
            cursor += header_size;
            if (length > compiled->size() - cursor || (opcode == 0x10 && length < 6)) throw std::runtime_error("compiled instruction extent");
            cursor += length;
        }
    }
    for (const auto* field : reference_fields) {
        const auto raw = static_cast<uint32_t>(integer(field->data, 0, 4));
        std::string status, form = "null", target = "null", variable = "null", runtime = "null";
        if (field->kind == "SCRV") {
            const auto found = first_variable.find(raw); status = found == first_variable.end() ? "missing_variable_declaration" : "dynamic_variable";
            if (found != first_variable.end()) variable = std::to_string(found->second);
        } else {
            const auto key = resolve(plugin.name, plugin.masters, raw);
            if (!key) status = "null_form";
            else {
                form = key->json(); const auto found = index.winners.find(*key);
                if (found != index.winners.end()) {
                    const auto& entry = found->second; const auto flags = integer(entry.header, 8, 4);
                    status = flags & 0x20 ? "deleted_form" : "defined_form";
                    target = object({{"source_plugin", quote(index.plugins[entry.source].name)}, {"record_file_offset", std::to_string(entry.offset)},
                        {"record_flags", std::to_string(flags)}, {"record_kind", quote(std::string(entry.header.begin(), entry.header.begin() + 4))}});
                } else if (key->origin == "falloutnv.esm" && key->local == 0x14) { status = "runtime_dependency"; runtime = quote("nv-player-reference"); }
                else status = "missing_form";
            }
        }
        ++counts.statuses[status];
        references.push_back(object({{"index", std::to_string(references.size() + 1)}, {"decoded_offset", std::to_string(field->offset)},
            {"source_kind", quote(field->kind)}, {"value", std::to_string(raw)}, {"status", quote(status)}, {"form_key", form}, {"target", target},
            {"variable_declaration_offset", variable}, {"runtime_dependency", runtime}}));
    }
    const auto& header = fields.front().data;
    if (integer(header, 4, 4) != references.size()) issues.push_back(quote("SCHR reference count differs from ordered table length"));
    if (integer(header, 8, 4) != (compiled ? compiled->size() : 0)) issues.push_back(quote("SCHR compiled size differs from SCDA extent"));
    const auto metadata_hash = fallout_tables::hash(metadata);
    const auto version = object({{"source_plugin", quote(plugin.name)}, {"source_sha256", quote(plugin.source->sha256)},
        {"record_file_offset", std::to_string(record.offset)}, {"record_flags", std::to_string(integer(record.header, 8, 4))},
        {"decoded_record_sha256", quote(body_hash)}, {"metadata_sha256", quote(metadata_hash)},
        {"compiled_sha256", compiled ? quote(fallout_tables::hash(*compiled)) : "null"}, {"compiled_bytes", compiled ? std::to_string(compiled->size()) : "null"}});
    const auto script_key = object({{"record", key.json()}, {"header_decoded_offset", std::to_string(marker)}});
    ++counts.numbers["scripts"]; counts.numbers["variables"] += variables.size(); counts.numbers["references"] += references.size();
    counts.numbers["scripts_with_issues"] += !issues.empty(); ++counts.owners[owner.kind]; ++counts.kinds[std::string(record.header.begin(), record.header.begin() + 4)];
    if (compiled) { ++counts.numbers["compiled_bodies"]; counts.numbers["compiled_bytes"] += compiled->size(); }
    if (counts.numbers["scripts"] > 262144 || counts.numbers["variables"] > 262144 || counts.numbers["references"] > 1000000) throw std::runtime_error("aggregate script metadata budget");
    return object({{"handle", object({{"key", script_key}, {"version_sha256", quote(version_hash(key, marker, plugin, record, body_hash, metadata_hash, compiled))}})},
        {"version", version}, {"owner", owner.json()}, {"script_type", std::to_string(integer(header, 16, 2))}, {"flags", std::to_string(integer(header, 18, 2))},
        {"declarations", array(variables)}, {"references", array(references)}, {"issues", array(issues)}});
}

#include "coverage.hpp"

int wmain(int argc, wchar_t** argv) {
    try {
        if (argc != 4) throw std::runtime_error("usage: script-catalogue-oracle Data_directory order_bundle decoded_catalogue_bundle");
        auto index = scan(argv[1], argv[2]);
        std::ifstream input(argv[3], std::ios::binary | std::ios::ate); const auto size = input.tellg();
        if (!input || size < 8 || size > 256 * 1024 * 1024) throw std::runtime_error("bundle budget");
        Bytes bundle(static_cast<size_t>(size)); input.seekg(0); input.read(reinterpret_cast<char*>(bundle.data()), size);
        if (!input) throw std::runtime_error("bundle read");
        const std::string magic(bundle.begin(), bundle.begin() + 8);
        if (magic == "FRCOVER1") {
            std::cout << object({{"schema_version", "1"}, {"profile", quote("nv-original")}, {"plugins", index.sources_json()},
                {"metadata", index.metadata_json()}, {"coverage", coverage(index, bundle)}, {"execution_ready", "false"}}) << '\n';
            return 0;
        }
        if (magic != "FRCAT001") throw std::runtime_error("bundle magic");
        size_t cursor = 8; Counts counts; std::set<Key> seen; std::vector<std::string> rows;
        for (; cursor < bundle.size();) {
            const auto source = static_cast<size_t>(integer(bundle, cursor, 1)); ++cursor;
            const auto name = take(bundle, cursor, 4); const std::string kind(name.begin(), name.end());
            const auto raw = static_cast<uint32_t>(integer(bundle, cursor, 4)); cursor += 4;
            const auto flags = static_cast<uint32_t>(integer(bundle, cursor, 4)); cursor += 4;
            const auto offset = integer(bundle, cursor, 8); cursor += 8;
            const auto length = static_cast<size_t>(integer(bundle, cursor, 4)); cursor += 4;
            if (source >= index.plugins.size() || length > 64 * 1024 * 1024 || !one_of(kind, {"SCPT", "INFO", "QUST", "PACK", "PERK", "TERM", "REFR", "ACHR", "ACRE", "PGRE", "PMIS", "PBEA"})) throw std::runtime_error("bundle source/kind/extent");
            const auto& plugin = index.plugins[source]; const auto key = resolve(plugin.name, plugin.masters, raw);
            if (!key || !seen.insert(*key).second) throw std::runtime_error("bundle null/duplicate identity");
            const auto found = index.winners.find(*key);
            if (found == index.winners.end()) throw std::runtime_error("bundle definition absent"); const auto& record = found->second;
            if (record.source != source || record.offset != offset || integer(record.header, 8, 4) != flags || integer(record.header, 12, 4) != raw
                || std::string(record.header.begin(), record.header.begin() + 4) != kind || (flags & 0x20)) throw std::runtime_error("bundle is not the live winning definition");
            const auto payload = take(bundle, cursor, length);
            if (!(flags & 0x40000)) {
                if (integer(record.header, 4, 4) != length || plugin.source->read(offset + 24, length) != payload) throw std::runtime_error("uncompressed source body differs");
            } else if (integer(record.header, 4, 4) < 4 || integer(plugin.source->read(offset + 24, 4), 0, 4) != length) throw std::runtime_error("compressed decoded extent differs");
            const auto units = fallout_tables::units(payload); if (units.empty()) throw std::runtime_error("bundle has no script unit");
            const auto ownership = owners(kind, fields(payload), units.size()); counts.numbers["source_ownership_findings"] += ownership.findings;
            ++counts.numbers["records_retained"]; counts.numbers["payload_bytes_retained"] += length;
            const auto body_hash = fallout_tables::hash(payload);
            for (const auto& unit : units) {
                const size_t marker = unit.front().offset; const auto owner = ownership.units.find(marker);
                rows.push_back(unit_json(index, *key, record, body_hash, unit, owner == ownership.units.end() ? Owner{"unverified_embedded", marker, {}, false} : owner->second, counts));
            }
        }
        std::cout << object({{"schema_version", "1"}, {"profile", quote("nv-original")}, {"plugins", index.sources_json()},
            {"metadata", index.metadata_json()}, {"catalogue_counts", counts.json()}, {"scripts", array(rows)},
            {"execution_ready", "false"}, {"live_event_lists_loaded", "false"}, {"retail_parity_accepted", "false"}}) << '\n';
        return 0;
    } catch (const std::exception& error) { std::cerr << "script-catalogue-oracle: " << error.what() << '\n'; return 1; }
}
