// Original offline quest/dialogue reader. It reads bytes, never game code.
#include "../oracle-common/pe_source.hpp"
#include "../oracle-common/table_source.hpp"
#include <set>

using fallout_oracle::Bytes;
using fallout_oracle::digest;
using fallout_tables::integer;
using fallout_tables::append;
using Object = std::map<std::string, std::string>;
using Index = std::optional<size_t>;

static std::string quote(const std::string& value) {
    std::ostringstream out; out << std::quoted(value); return out.str();
}
static std::string object(const Object& fields) {
    std::string result = "{"; bool comma = false;
    for (const auto& field : fields) { if (comma) result += ','; comma = true; result += quote(field.first) + ':' + field.second; }
    return result + '}';
}
static std::string array(const std::vector<std::string>& values) {
    std::string result = "["; bool comma = false;
    for (const auto& value : values) { if (comma) result += ','; comma = true; result += value; } return result + ']';
}
static std::string index_json(Index value) { return value ? std::to_string(*value) : "null"; }
static bool one_of(const std::string& value, std::initializer_list<const char*> values) {
    return std::any_of(values.begin(), values.end(), [&](const auto item) { return value == item; });
}
static Bytes take(const Bytes& data, size_t& cursor, size_t length) {
    if (cursor > data.size() || length > data.size() - cursor) throw std::runtime_error("truncated bytes");
    Bytes result(data.begin() + cursor, data.begin() + cursor + length); cursor += length; return result;
}
static void append_digest(Bytes& data, const Bytes& source) {
    const auto hex = digest(source);
    for (size_t at = 0; at < hex.size(); at += 2) data.push_back(static_cast<unsigned char>(std::stoul(hex.substr(at, 2), nullptr, 16)));
}

struct Section {
    std::string kind; Index offset, parent; std::optional<int64_t> key;
    std::string json() const { return object({{"kind", quote(kind)}, {"marker_offset", index_json(offset)},
        {"parent", index_json(parent)}, {"key", key ? std::to_string(*key) : "null"}}); }
};
struct Decoded { unsigned char tag = 0; std::vector<uint64_t> words; std::optional<int64_t> key; };
static Decoded decode(const std::string& record, const std::string& kind, const Bytes& data) {
    auto length = [&](std::initializer_list<size_t> sizes) {
        if (std::find(sizes.begin(), sizes.end(), data.size()) == sizes.end()) throw std::runtime_error("invalid " + record + '/' + kind + " length");
    };
    auto form = [&] { length({4}); return Decoded{2, {integer(data, 0, 4)}, static_cast<int64_t>(integer(data, 0, 4))}; };
    auto optional = [&](Decoded& result, size_t at, size_t width) {
        const bool present = data.size() >= at + width;
        result.words.push_back(present); result.words.push_back(present ? integer(data, at, width) : 0);
    };
    if (kind == "CTDA") {
        length({20,24,28}); Decoded result{12};
        for (size_t at = 0; at < 4; ++at) result.words.push_back(data[at]);
        result.words.push_back(integer(data, 4, 4)); result.words.push_back(integer(data, 8, 2));
        result.words.push_back(data[10]); result.words.push_back(data[11]);
        result.words.push_back(integer(data, 12, 4)); result.words.push_back(integer(data, 16, 4));
        optional(result, 20, 4); optional(result, 24, 4); return result;
    }
    if (kind == "DATA") {
        if (record == "QUST") {
            length({2,4,8}); Decoded result{6, {data[0], data[1], uint64_t(data.size() >= 4),
                data.size() >= 4 ? uint64_t(data[2]) : 0, data.size() >= 4 ? uint64_t(data[3]) : 0}};
            optional(result, 4, 4); return result;
        }
        if (record == "INFO") { length({3,4}); Decoded result{7, {data[0], data[1], data[2]}}; optional(result, 3, 1); return result; }
        length({1,2}); Decoded result{8, {data[0]}}; optional(result, 1, 1); return result;
    }
    if (record == "QUST") {
        if (kind == "INDX" || kind == "QOBJ") {
            length({kind == "INDX" ? size_t(2) : size_t(4)});
            const int64_t value = kind == "INDX" ? static_cast<int16_t>(integer(data, 0, 2)) : static_cast<int32_t>(integer(data, 0, 4));
            return {3, {static_cast<uint64_t>(value)}, value};
        }
        if (kind == "QSDT") { length({1}); return {5, {data[0]}}; }
        if (kind == "QSTA") { length({8}); return {10, {integer(data, 0, 4), data[4], data[5], data[6], data[7]}}; }
        if (one_of(kind, {"SCRI", "NAM0"})) return form();
        if (one_of(kind, {"EDID", "FULL", "ICON", "CNAM", "NNAM"})) return {1};
    } else if (record == "INFO") {
        if (kind == "TRDT") {
            length({20,24}); Decoded result{9, {integer(data, 0, 4), static_cast<uint64_t>(static_cast<int64_t>(static_cast<int32_t>(integer(data, 4, 4))))}};
            for (size_t at = 8; at < 16; ++at) result.words.push_back(data[at]);
            result.words.push_back(integer(data, 16, 4)); optional(result, 20, 1);
            for (size_t at = 21; at < 24; ++at) result.words.push_back(data.size() == 24 ? data[at] : 0);
            return result;
        }
        if (kind == "NEXT") { length({0}); return {11}; }
        if (kind == "DNAM") { length({4}); return {3, {integer(data, 0, 4)}}; }
        if (one_of(kind, {"QSTI", "TPIC", "PNAM", "NAME", "SNAM", "LNAM", "TCLT", "TCLF", "TCFU", "SNDD", "ANAM", "KNAM"})) return form();
        if (one_of(kind, {"NAM1", "NAM2", "NAM3", "RNAM"})) return {1};
    } else {
        if (kind == "PNAM") { length({4}); return {4, {integer(data, 0, 4)}}; }
        if (kind == "INFX") { length({4}); return {3, {static_cast<uint64_t>(static_cast<int64_t>(static_cast<int32_t>(integer(data, 0, 4))))}}; }
        if (one_of(kind, {"QSTI", "INFC", "QSTR"})) return form();
        if (one_of(kind, {"EDID", "FULL", "TDUM"})) return {1};
    }
    return {};
}

struct Counts {
    std::map<std::string, uint64_t> numbers, records, fields, sections, conditions, scripts;
    std::map<std::string, std::map<size_t, uint64_t>> lengths;
    Counts() { for (const auto key : {"records", "fields", "sections", "conditions", "script_units", "compiled_bodies", "compiled_bytes", "findings", "unowned_fields"}) numbers[key] = 0; }
    std::string json() const {
        auto values = [](const auto& source) { Object result; for (const auto& item : source) result[item.first] = std::to_string(item.second); return object(result); };
        Object result; for (const auto& item : numbers) result[item.first] = std::to_string(item.second);
        result["record_kinds"] = values(records); result["field_kinds"] = values(fields); result["section_kinds"] = values(sections);
        result["condition_owners"] = values(conditions); result["script_owners"] = values(scripts);
        Object field_lengths; for (const auto& field : lengths) { Object rows; for (const auto& item : field.second) rows[std::to_string(item.first)] = std::to_string(item.second); field_lengths[field.first] = object(rows); }
        result["field_lengths"] = object(field_lengths); return object(result);
    }
};

static std::string inspect(const std::string& record, uint32_t form, uint32_t flags, uint64_t file_offset,
    const Bytes& payload, Counts& totals) {
    const std::string root = record == "QUST" ? "quest" : record == "INFO" ? "dialogue_info" : "dialogue_topic";
    std::vector<Section> sections{{root, {}, {}, {}}}; std::vector<std::string> findings;
    std::map<size_t, Index> owners;
    Index stage, entry, objective, target, response, script, quest, connection;
    bool end_script = false; size_t begin_count = 0, end_count = 0;
    Bytes field_tuples; size_t field_count = 0;
    Index extended;
    for (size_t cursor = 0; cursor < payload.size();) {
        if (payload.size() - cursor < 6) throw std::runtime_error("truncated field");
        const size_t offset = cursor;
        const std::string kind(payload.begin() + cursor, payload.begin() + cursor + 4);
        const size_t short_size = static_cast<size_t>(integer(payload, cursor + 4, 2)); cursor += 6;
        if (kind == "XXXX") { if (extended || short_size != 4) throw std::runtime_error("XXXX shape"); extended = static_cast<size_t>(integer(payload, cursor, 4)); cursor += 4; continue; }
        if (++field_count > 1048576) throw std::runtime_error("field budget");
        const auto data = take(payload, cursor, extended.value_or(short_size)); extended.reset();
        const auto decoded = decode(record, kind, data);
        Index owner; std::string issue;
        auto child = [&](const std::string& name, Index parent, std::optional<int64_t> key = {}) {
            if (sections.size() >= 65536) throw std::runtime_error("section budget");
            owner = sections.size(); sections.push_back({name, offset, parent, key}); if (!parent) issue = "missing authored parent";
        };
        auto assigned = [&](Index index) { owner = index; if (!owner) issue = "missing authored owner"; };
        if (record == "QUST") {
            if (kind == "INDX") { child("stage", 0, decoded.key); stage = owner; entry.reset(); objective.reset(); target.reset(); }
            else if (kind == "QSDT") { child("log_entry", stage); entry = owner; }
            else if (kind == "QOBJ") { child("objective", 0, decoded.key); objective = owner; stage.reset(); entry.reset(); target.reset(); }
            else if (kind == "QSTA") { child("target", objective); target = owner; }
            else if (one_of(kind, {"CNAM", "NAM0"}) || fallout_tables::selected(kind)) assigned(entry);
            else if (kind == "NNAM") assigned(objective);
            else if (kind == "CTDA") assigned((target || objective) ? target : (entry || stage) ? entry : Index(0));
            else if (one_of(kind, {"EDID", "SCRI", "FULL", "ICON", "DATA"})) { owner = 0; if (kind == "DATA" && data.size() == 2) issue = "short quest header; loading unverified"; }
            else issue = "unrecognized field";
        } else if (record == "INFO") {
            if (kind == "TRDT") { script.reset(); child("response", 0); response = owner; }
            else if (one_of(kind, {"NAM1", "NAM2", "NAM3", "SNAM", "LNAM"})) assigned(response);
            else if (kind == "NEXT") { response.reset(); script.reset(); end_script = true; owner = 0; }
            else if (kind == "SCHR") {
                response.reset(); child(end_script ? "end_script" : "begin_script", 0); script = owner;
                auto& count = end_script ? end_count : begin_count; if (count++) issue = "repeated script role";
            } else if (fallout_tables::selected(kind)) assigned(script);
            else if (one_of(kind, {"CTDA", "DATA", "QSTI", "TPIC", "PNAM", "NAME", "TCLT", "TCLF", "TCFU", "SNDD", "RNAM", "ANAM", "KNAM", "DNAM"})) { response.reset(); script.reset(); owner = 0; }
            else issue = "unrecognized field";
        } else {
            if (kind == "QSTI") { child("added_quest", 0, decoded.key); quest = owner; connection.reset(); }
            else if (kind == "INFC") { child("info_connection", quest, decoded.key); connection = owner; }
            else if (kind == "INFX") assigned(connection);
            else if (one_of(kind, {"EDID", "QSTR", "FULL", "PNAM", "TDUM", "DATA"})) { quest.reset(); connection.reset(); owner = 0; }
            else issue = "unrecognized field";
        }
        if (!issue.empty()) { if (findings.size() >= 65536) throw std::runtime_error("finding budget"); findings.push_back(object({{"field_offset", std::to_string(offset)}, {"reason", quote(issue)}})); }
        owners[offset] = owner;
        append(field_tuples, offset, 4); field_tuples.insert(field_tuples.end(), kind.begin(), kind.end());
        append(field_tuples, data.size(), 4); append(field_tuples, owner.value_or(UINT32_MAX), 4); append(field_tuples, decoded.tag, 1);
        append(field_tuples, decoded.words.size(), 4); for (const auto word : decoded.words) append(field_tuples, word, 8); append_digest(field_tuples, data);
        ++totals.fields[kind]; ++totals.lengths[kind][data.size()]; totals.numbers["unowned_fields"] += !owner;
        if (decoded.tag == 12) { ++totals.numbers["conditions"]; ++totals.conditions[owner ? sections[*owner].kind : "unowned"]; }
    }
    if (extended) throw std::runtime_error("orphan XXXX");
    std::vector<std::string> scripts;
    for (const auto& unit : fallout_tables::units(payload)) {
        const auto table = fallout_tables::inspect(unit); const auto owner = owners.at(unit.front().offset);
        ++totals.numbers["script_units"]; ++totals.scripts[owner ? sections[*owner].kind : "unowned"];
        if (table.compiled) { ++totals.numbers["compiled_bodies"]; totals.numbers["compiled_bytes"] += table.compiled->size(); }
        scripts.push_back(object({{"header_decoded_offset", std::to_string(unit.front().offset)}, {"owner", index_json(owner)},
            {"metadata_sha256", quote(digest(table.metadata))}, {"compiled_bytes", table.compiled ? std::to_string(table.compiled->size()) : "null"},
            {"compiled_sha256", table.compiled ? quote(digest(*table.compiled)) : "null"}}));
    }
    std::vector<std::string> section_rows; for (const auto& section : sections) { section_rows.push_back(section.json()); ++totals.sections[section.kind]; }
    ++totals.numbers["records"]; ++totals.records[record]; totals.numbers["fields"] += field_count;
    totals.numbers["sections"] += sections.size(); totals.numbers["findings"] += findings.size();
    return object({{"record_kind", quote(record)}, {"form_id", std::to_string(form)}, {"record_file_offset", std::to_string(file_offset)},
        {"record_flags", std::to_string(flags)}, {"decoded_bytes", std::to_string(payload.size())}, {"decoded_sha256", quote(digest(payload))},
        {"fields", std::to_string(field_count)}, {"fields_sha256", quote(digest(field_tuples))},
        {"sections", array(section_rows)}, {"scripts", array(scripts)}, {"findings", array(findings)}});
}

int wmain(int argc, wchar_t** argv) {
    try {
        if (argc != 3) throw std::runtime_error("usage: narrative-oracle decoded-record-bundle FalloutNV.exe");
        const fallout_oracle::SourceImage source(argv[2]);
        std::ifstream input(argv[1], std::ios::binary | std::ios::ate); const auto size = input.tellg();
        if (!input || size < 8 || size > 256 * 1024 * 1024) throw std::runtime_error("bundle byte budget");
        Bytes bundle(static_cast<size_t>(size)); input.seekg(0); input.read(reinterpret_cast<char*>(bundle.data()), size);
        if (!input || std::string(bundle.begin(), bundle.begin() + 8) != "FRNARR01") throw std::runtime_error("bundle magic/read");
        Counts totals; std::vector<std::string> rows;
        for (size_t cursor = 8; cursor < bundle.size();) {
            if (rows.size() >= 262144) throw std::runtime_error("record budget");
            const auto signature = take(bundle, cursor, 4); const std::string kind(signature.begin(), signature.end());
            if (!one_of(kind, {"QUST", "INFO", "DIAL"})) throw std::runtime_error("record kind");
            const auto form = static_cast<uint32_t>(integer(bundle, cursor, 4)); cursor += 4;
            const auto flags = static_cast<uint32_t>(integer(bundle, cursor, 4)); cursor += 4;
            const auto file_offset = integer(bundle, cursor, 8); cursor += 8;
            const auto length = static_cast<size_t>(integer(bundle, cursor, 4)); cursor += 4;
            if (length > 64 * 1024 * 1024) throw std::runtime_error("record byte budget");
            const auto payload = take(bundle, cursor, length);
            rows.push_back(inspect(kind, form, flags, file_offset, payload, totals));
            if (totals.numbers["fields"] > 8000000 || totals.numbers["sections"] > 2000000 || totals.numbers["findings"] > 262144) throw std::runtime_error("aggregate budget");
        }
        std::cout << object({{"schema_version", "1"}, {"bundle_sha256", quote(digest(bundle))},
            {"executable_source_sha256", quote(source.sha256)}, {"rows", array(rows)}, {"counts", totals.json()}, {"execution_ready", "false"}}) << '\n';
        return 0;
    } catch (const std::exception& error) { std::cerr << "narrative-oracle: " << error.what() << '\n'; return 1; }
}
