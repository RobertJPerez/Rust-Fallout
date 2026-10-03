// Original static quest/declaration comparison. Definition selection comes from
// original locked headers. Event lists, variable values and handlers are absent.
#include "../oracle-common/operand_binding_source.hpp"
#include "../oracle-common/record_source.hpp"
namespace records = fallout_records;
using fallout_tables::integer;

struct DecodedRecord { records::Key key; records::Entry entry; Bytes payload; };
static std::vector<DecodedRecord> read_records(const records::HeaderIndex& index, const std::filesystem::path& path, const std::string& magic) {
    std::ifstream input(path, std::ios::binary | std::ios::ate); const auto size = input.tellg();
    if (!input || size < 8 || size > 256 * 1024 * 1024) throw std::runtime_error("record bundle byte budget");
    Bytes bytes(static_cast<size_t>(size)); input.seekg(0); input.read(reinterpret_cast<char*>(bytes.data()), size);
    if (!input || std::string(bytes.begin(), bytes.begin() + 8) != magic) throw std::runtime_error("record bundle magic");
    std::set<records::Key> seen; std::vector<DecodedRecord> result;
    for (size_t cursor = 8; cursor < bytes.size();) {
        const auto source_index = static_cast<size_t>(integer(bytes, cursor, 1)); ++cursor;
        const auto name = take(bytes, cursor, 4); const std::string kind(name.begin(), name.end());
        const auto raw = static_cast<uint32_t>(integer(bytes, cursor, 4)); cursor += 4;
        const auto flags = static_cast<uint32_t>(integer(bytes, cursor, 4)); cursor += 4;
        const auto offset = integer(bytes, cursor, 8); cursor += 8;
        const auto length = static_cast<size_t>(integer(bytes, cursor, 4)); cursor += 4;
        if (source_index >= index.plugins.size() || length > 64 * 1024 * 1024 || result.size() >= 262144) throw std::runtime_error("record source/extent/count budget");
        const auto& plugin = index.plugins[source_index]; const auto key = records::resolve(plugin.name, plugin.masters, raw);
        if (!key || !seen.insert(*key).second) throw std::runtime_error("record null/duplicate identity");
        const auto found = index.winners.find(*key); if (found == index.winners.end()) throw std::runtime_error("record definition absent");
        const auto& entry = found->second;
        if (entry.source != source_index || entry.offset != offset || integer(entry.header, 8, 4) != flags || integer(entry.header, 12, 4) != raw
            || std::string(entry.header.begin(), entry.header.begin() + 4) != kind || (flags & 0x20)) throw std::runtime_error("record differs from live winning header");
        auto payload = take(bytes, cursor, length); const auto stored = integer(entry.header, 4, 4);
        if (!(flags & 0x40000)) {
            if (stored != length || plugin.source->read(offset + 24, length) != payload) throw std::runtime_error("uncompressed source body differs");
        } else if (stored < 4 || integer(plugin.source->read(offset + 24, 4), 0, 4) != length) throw std::runtime_error("compressed source extent differs");
        if (magic == "FRQUEST1" && kind != "QUST") throw std::runtime_error("quest bundle has another kind");
        result.push_back({*key, entry, std::move(payload)});
    }
    return result;
}
static void version_text(Bytes& bytes, const std::string& value) {
    fallout_tables::append(bytes, value.size(), 4); bytes.insert(bytes.end(), value.begin(), value.end());
}
struct Definition {
    records::Key key; records::Entry entry; size_t marker; fallout_tables::Summary table; std::string version;
    std::string key_json() const { return object({{"record", key.json()}, {"header_decoded_offset", std::to_string(marker)}}); }
    std::string handle_json() const { return object({{"key", key_json()}, {"version_sha256", quote_json(version)}}); }
};
using Definitions = std::map<records::Key, std::vector<Definition>>;
static Definitions load_definitions(const records::HeaderIndex& index, const std::vector<DecodedRecord>& source) {
    Definitions result; size_t count = 0;
    for (const auto& record : source) {
        const auto units = fallout_tables::units(record.payload); if (units.empty()) throw std::runtime_error("script bundle lacks units");
        const auto& plugin = index.plugins[record.entry.source]; const auto body = digest(record.payload);
        for (const auto& unit : units) {
            if (++count > 262144) throw std::runtime_error("definition budget");
            auto table = fallout_tables::inspect(unit); const size_t marker = unit.front().offset;
            Bytes version{'F','R','S','C','R','V','0','1'};
            version_text(version, "nv-original"); version_text(version, record.key.origin); fallout_tables::append(version, record.key.local, 4); fallout_tables::append(version, marker, 4);
            version_text(version, plugin.name); version_text(version, plugin.source->sha256); fallout_tables::append(version, record.entry.offset, 8);
            fallout_tables::append(version, integer(record.entry.header, 8, 4), 4); version_text(version, body); version_text(version, digest(table.metadata));
            fallout_tables::append(version, table.compiled.has_value(), 1);
            if (table.compiled) { version_text(version, digest(*table.compiled)); fallout_tables::append(version, table.compiled->size(), 8); }
            result[record.key].push_back({record.key, record.entry, marker, std::move(table), digest(version)});
        }
    }
    return result;
}
static std::string declaration_json(uint32_t index, const fallout_tables::Variable& variable) {
    return object({{"index", std::to_string(index)}, {"type_byte", std::to_string(variable.declaration[16])}, {"decoded_offset", std::to_string(variable.offset)},
        {"name_bytes", std::to_string(variable.name.size())}, {"name_sha256", quote_json(digest(variable.name))}});
}
static bool one_of(const std::string& value, std::initializer_list<const char*> values) {
    return std::any_of(values.begin(), values.end(), [&](const auto item) { return value == item; });
}
struct QuestFields { std::vector<std::string> rows, findings; std::vector<std::optional<records::Key>> scripts; };
static QuestFields quest_fields(const records::Plugin& plugin, const Bytes& bytes) {
    QuestFields result; std::optional<size_t> extended, stage, entry, objective, target; size_t fields = 0;
    for (size_t cursor = 0; cursor < bytes.size();) {
        const size_t offset = cursor; const auto name = take(bytes, cursor, 4); const std::string kind(name.begin(), name.end());
        const auto short_size = static_cast<size_t>(integer(bytes, cursor, 2)); cursor += 2;
        if (kind == "XXXX") { if (extended || short_size != 4) throw std::runtime_error("quest XXXX"); extended = static_cast<size_t>(integer(bytes, cursor, 4)); cursor += 4; continue; }
        if (++fields > 1048576) throw std::runtime_error("quest field budget");
        const auto data = take(bytes, cursor, extended.value_or(short_size)); extended.reset(); std::string finding;
        if (kind == "SCRI") {
            if (data.size() != 4) throw std::runtime_error("quest SCRI extent"); const auto raw = static_cast<uint32_t>(integer(data, 0, 4)); const auto key = records::resolve(plugin.name, plugin.masters, raw);
            result.scripts.push_back(key); result.rows.push_back(object({{"decoded_offset", std::to_string(offset)}, {"raw_form", std::to_string(raw)}, {"key", key ? key->json() : "null"}}));
        } else if (kind == "INDX") { if (data.size() != 2) throw std::runtime_error("stage extent"); stage = offset; entry.reset(); objective.reset(); target.reset(); }
        else if (kind == "QSDT") { if (data.size() != 1) throw std::runtime_error("log entry extent"); entry = offset; if (!stage) finding = "missing authored parent"; }
        else if (kind == "QOBJ") { if (data.size() != 4) throw std::runtime_error("objective extent"); objective = offset; stage.reset(); entry.reset(); target.reset(); }
        else if (kind == "QSTA") { if (data.size() != 8) throw std::runtime_error("quest target extent"); target = offset; if (!objective) finding = "missing authored parent"; }
        else if (one_of(kind, {"CNAM", "NAM0"}) || fallout_tables::selected(kind)) { if (!entry) finding = "missing authored owner"; }
        else if (kind == "NNAM") { if (!objective) finding = "missing authored owner"; }
        else if (kind == "CTDA") {
            if (data.size() != 20 && data.size() != 24 && data.size() != 28) throw std::runtime_error("quest condition extent");
            if ((target || objective) ? !target : (entry || stage) ? !entry : false) finding = "missing authored owner";
        } else if (kind == "DATA") {
            if (data.size() != 2 && data.size() != 4 && data.size() != 8) throw std::runtime_error("quest DATA extent");
            if (data.size() == 2) finding = "short quest header; loading unverified";
        } else if (!one_of(kind, {"EDID", "FULL", "ICON"})) finding = "unrecognized field";
        if (!finding.empty()) {
            if (result.findings.size() >= 65536) throw std::runtime_error("quest finding budget");
            result.findings.push_back(object({{"field_decoded_offset", std::to_string(offset)}, {"reason", quote_json(finding)}}));
        }
    }
    if (extended) throw std::runtime_error("orphan quest XXXX"); return result;
}
struct Attachment { std::string status, json; const Definition* script = nullptr; };
using Attachments = std::map<records::Key, Attachment>;
struct QuestCounts {
    uint64_t quests = 0, bytes = 0, fields = 0, findings = 0; std::map<std::string, uint64_t> statuses;
    std::string json() const { Object values; for (const auto& item : statuses) values[item.first] = std::to_string(item.second);
        return object({{"quests", std::to_string(quests)}, {"decoded_bytes", std::to_string(bytes)}, {"fields", std::to_string(fields)}, {"source_findings", std::to_string(findings)}, {"statuses", object(values)}}); }
};
static Attachments load_quests(const records::HeaderIndex& index, const Definitions& definitions, const std::vector<DecodedRecord>& source, QuestCounts& counts) {
    std::map<records::Key, const DecodedRecord*> decoded; for (const auto& record : source) decoded.emplace(record.key, &record);
    Attachments result;
    for (const auto& item : index.winners) {
        const auto& key = item.first; const auto& entry = item.second;
        if (std::string(entry.header.begin(), entry.header.begin() + 4) != "QUST") continue;
        if (++counts.quests > 65536) throw std::runtime_error("quest record budget");
        const auto& plugin = index.plugins[entry.source]; const auto flags = integer(entry.header, 8, 4); Attachment attachment{"deleted_quest", "", nullptr};
        QuestFields fields; std::string body = "null";
        if (!(flags & 0x20)) {
            const auto found = decoded.find(key); if (found == decoded.end()) throw std::runtime_error("missing winning quest payload");
            counts.bytes += found->second->payload.size(); if (counts.bytes > 256 * 1024 * 1024) throw std::runtime_error("quest decoded byte budget");
            body = quote_json(digest(found->second->payload)); fields = quest_fields(plugin, found->second->payload);
            if (fields.scripts.empty()) attachment.status = "no_script_field";
            else if (fields.scripts.size() != 1) attachment.status = "multiple_script_fields";
            else if (!fields.scripts[0]) attachment.status = "null_script";
            else {
                const auto target = index.winners.find(*fields.scripts[0]);
                if (target == index.winners.end()) attachment.status = "missing_script";
                else if (integer(target->second.header, 8, 4) & 0x20) attachment.status = "deleted_script";
                else if (std::string(target->second.header.begin(), target->second.header.begin() + 4) != "SCPT") attachment.status = "wrong_script_kind";
                else {
                    const auto scripts = definitions.find(*fields.scripts[0]);
                    if (scripts == definitions.end()) attachment.status = "missing_loaded_definition";
                    else if (scripts->second.size() != 1) attachment.status = "multiple_standalone_units";
                    else { attachment.status = "loaded_definition"; attachment.script = &scripts->second.front(); }
                }
            }
        }
        counts.fields += fields.rows.size(); counts.findings += fields.findings.size(); ++counts.statuses[attachment.status];
        attachment.json = object({{"quest", key.json()}, {"source", object({{"plugin", quote_json(plugin.name)}, {"sha256", quote_json(plugin.source->sha256)},
            {"record_file_offset", std::to_string(entry.offset)}, {"record_flags", std::to_string(flags)}, {"decoded_record_sha256", body}})},
            {"fields", array(fields.rows)}, {"status", quote_json(attachment.status)}, {"script", attachment.script ? attachment.script->handle_json() : "null"}, {"findings", array(fields.findings)}});
        result.emplace(key, std::move(attachment));
    }
    if (decoded.size() != counts.quests - counts.statuses["deleted_quest"]) throw std::runtime_error("quest payload coverage differs");
    // Counting an absent deleted status must not manufacture a serialized zero.
    if (!counts.statuses["deleted_quest"]) counts.statuses.erase("deleted_quest");
    return result;
}
static std::string foreign(const records::HeaderIndex& index, const Attachments& quests, const Definition& source, const Use& use, std::string& status) {
    std::string form = "null", quest_status = "null", target_script = "null", declaration = "null";
    const auto& plugin = index.plugins[source.entry.source]; const auto& reference = source.table.references.at(*use.context - 1);
    if (reference.kind) status = source.table.variables.count(reference.target) ? "dynamic_context" : "missing_context_variable";
    else {
        const auto key = records::resolve(plugin.name, plugin.masters, reference.target);
        if (!key) status = "null_context";
        else {
            form = key->json(); const auto found = index.winners.find(*key);
            if (found == index.winners.end()) status = key->origin == "falloutnv.esm" && key->local == 0x14 ? "runtime_context" : "missing_context_form";
            else if (integer(found->second.header, 8, 4) & 0x20) status = "deleted_context_form";
            else {
                const auto kind = std::string(found->second.header.begin(), found->second.header.begin() + 4);
                if (one_of(kind, {"REFR", "ACHR", "ACRE", "PGRE", "PMIS", "PBEA"})) status = "placed_reference_needs_event_list";
                else if (kind != "QUST") status = "unsupported_context_kind";
                else {
                    const auto quest = quests.find(*key);
                    if (quest == quests.end()) status = "missing_quest_attachment";
                    else {
                        quest_status = quote_json(quest->second.status);
                        if (!quest->second.script) status = "quest_script_unavailable";
                        else {
                            target_script = quest->second.script->handle_json(); const auto local = quest->second.script->table.variables.find(use.index);
                            status = local == quest->second.script->table.variables.end() ? "missing_foreign_declaration" : "static_quest_declaration";
                            if (local != quest->second.script->table.variables.end()) declaration = declaration_json(use.index, local->second);
                        }
                    }
                }
            }
        }
    }
    return object({{"scda_offset", std::to_string(use.offset)}, {"role", std::to_string(use.role)}, {"lookup", object({{"source_script", source.key_json()},
        {"context_reference", std::to_string(*use.context)}, {"local_index", std::to_string(use.index)}, {"status", quote_json(status)}, {"context_form", form},
        {"quest_attachment_status", quest_status}, {"target_script", target_script}, {"declaration", declaration}, {"live_value_resolved", "false"}})}});
}

int wmain(int argc, wchar_t** argv) {
    try {
        if (argc != 6) throw std::runtime_error("usage: quest-script-oracle Data_directory order_bundle script_bundle quest_bundle FalloutNV.exe");
        setlocale(LC_NUMERIC, "C"); auto index = records::scan(argv[1], argv[2]); records::Source executable_guard(argv[5]); const SourceImage executable(argv[5]);
        std::vector<std::string> operators_json; const auto ops = operators(executable, operators_json); const auto signatures_table = signatures(executable);
        const auto script_records = read_records(index, argv[3], "FRCAT001"); const auto definitions = load_definitions(index, script_records);
        const auto quest_records = read_records(index, argv[4], "FRQUEST1"); QuestCounts quest_counts; const auto quests = load_quests(index, definitions, quest_records, quest_counts);
        std::vector<std::string> quest_rows, units, foreign_rows; uint64_t uses = 0; std::map<std::string, uint64_t> statuses;
        for (const auto& item : quests) quest_rows.push_back(item.second.json);
        for (const auto& record : definitions) for (const auto& unit : record.second) {
            if (!unit.table.compiled) continue;
            const auto binding = bind(unit.table, ops, signatures_table); uses += binding.uses; if (uses > 2000000 || units.size() >= 262144) throw std::runtime_error("loaded operand budget");
            const auto first = foreign_rows.size();
            for (const auto& use : binding.decoded_uses) if (use.context && use.status == 4) {
                if (foreign_rows.size() >= 1000000) throw std::runtime_error("foreign operand budget");
                std::string status; foreign_rows.push_back(foreign(index, quests, unit, use, status)); ++statuses[status];
            }
            units.push_back(object({{"handle", unit.handle_json()}, {"binding_sha256", quote_json(digest(binding.tuples))}, {"counts", binding.counts_json()},
                {"decode_issues", "[]"}, {"foreign_uses", std::to_string(foreign_rows.size() - first)}}));
        }
        Object status_json; for (const auto& item : statuses) status_json[item.first] = std::to_string(item.second);
        std::cout << object({{"schema_version", "1"}, {"profile", quote_json("nv-original")}, {"plugins", index.sources_json()}, {"metadata", index.metadata_json()},
            {"quest_counts", quest_counts.json()}, {"quests", array(quest_rows)}, {"compiled_units", array(units)}, {"foreign_operands", array(foreign_rows)},
            {"operand_counts", object({{"uses", std::to_string(uses)}, {"missing_bindings", "0"}, {"decode_issues", "0"}, {"foreign_uses", std::to_string(foreign_rows.size())}, {"declaration_statuses", object(status_json)}})},
            {"executable_sha256", quote_json(executable.sha256)}, {"static_declarations_only", "true"}, {"live_event_lists_loaded", "false"},
            {"live_values_resolved", "false"}, {"execution_ready", "false"}, {"retail_parity_accepted", "false"}}) << '\n';
        return 0;
    } catch (const std::exception& error) { std::cerr << "quest-script-oracle: " << error.what() << '\n'; return 1; }
}
