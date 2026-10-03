// Original offline CTDA field reader. No migration, grouping or evaluation.
#include "../oracle-common/pe_source.hpp"
#include <iostream>
#include <map>
#include <optional>

using fallout_oracle::Bytes;
using fallout_oracle::SourceImage;
using fallout_oracle::digest;
using Object = std::map<std::string, std::string>;

static std::string quote_json(const std::string& text) {
    std::ostringstream out; out << std::quoted(text); return out.str();
}
static std::string object(const Object& fields) {
    std::string out = "{"; bool comma = false;
    for (const auto& field : fields) { if (comma) out += ','; comma = true; out += quote_json(field.first) + ':' + field.second; }
    return out + '}';
}
static std::string array(const std::vector<std::string>& rows) {
    std::string out = "["; bool comma = false;
    for (const auto& row : rows) { if (comma) out += ','; comma = true; out += row; } return out + ']';
}
static uint64_t integer(const Bytes& bytes, size_t at, size_t width) {
    if (at > bytes.size() || width > bytes.size() - at || width > 8) throw std::runtime_error("truncated integer");
    uint64_t result = 0; for (size_t i = 0; i < width; ++i) result |= uint64_t(bytes[at + i]) << (i * 8); return result;
}
static Bytes take(const Bytes& bytes, size_t& at, size_t size) {
    if (at > bytes.size() || size > bytes.size() - at) throw std::runtime_error("truncated payload");
    Bytes result(bytes.begin() + at, bytes.begin() + at + size); at += size; return result;
}
template<class T> static std::string counts(const std::map<T, uint64_t>& values) {
    Object result; for (const auto& entry : values) result[std::to_string(entry.first)] = std::to_string(entry.second); return object(result);
}

struct Counts {
    std::map<std::string, uint64_t> numbers;
    std::map<uint32_t, uint64_t> lengths, functions, flags, operators, subjects, groups;
    std::map<std::string, uint64_t> record_kinds;
    Counts() {
        for (const auto key : {"conditions", "or_flags", "global_comparisons", "nonfinite_float_comparisons",
            "nonzero_flag_padding", "nonzero_function_padding", "uninterpreted_low_flags", "unknown_comparison_operators",
            "absent_run_on_words", "absent_reference_words", "active_reference_words", "unverified_subject_selector_words"}) numbers[key] = 0;
    }
    std::string json() const {
        Object result; for (const auto& number : numbers) result[number.first] = std::to_string(number.second);
        result["lengths"] = counts(lengths); result["functions"] = counts(functions); result["flags"] = counts(flags);
        result["comparison_operators"] = counts(operators); result["subject_run_on_words"] = counts(subjects);
        result["animation_group_words"] = counts(groups);
        Object kinds; for (const auto& kind : record_kinds) kinds[kind.first] = std::to_string(kind.second);
        result["record_kinds"] = object(kinds); return object(result);
    }
};

static std::string condition(const Bytes& data, const std::string& kind, uint32_t form, uint64_t file_offset,
    size_t field_offset, const std::optional<std::pair<std::string, size_t>>& previous, Counts& totals) {
    if (data.size() != 20 && data.size() != 24 && data.size() != 28) throw std::runtime_error("CTDA layout");
    const auto flags = data[0]; const auto comparison = static_cast<uint32_t>(integer(data, 4, 4));
    const auto function = static_cast<uint16_t>(integer(data, 8, 2));
    const bool group = function == 106 || function == 285;
    const std::optional<uint32_t> run = data.size() >= 24 ? std::optional<uint32_t>(static_cast<uint32_t>(integer(data, 20, 4))) : std::nullopt;
    const std::optional<uint32_t> reference = data.size() >= 28 ? std::optional<uint32_t>(static_cast<uint32_t>(integer(data, 24, 4))) : std::nullopt;
    const bool selector = !group && run == 2;
    const bool global = (flags & 4) != 0;
    const bool flag_padding = data[1] || data[2] || data[3], function_padding = data[10] || data[11];
    ++totals.numbers["conditions"]; ++totals.record_kinds[kind]; ++totals.lengths[static_cast<uint32_t>(data.size())];
    ++totals.functions[function]; ++totals.flags[flags]; ++totals.operators[flags >> 5];
    totals.numbers["or_flags"] += (flags & 1) != 0; totals.numbers["global_comparisons"] += global;
    totals.numbers["nonfinite_float_comparisons"] += !global && (comparison & 0x7f800000) == 0x7f800000;
    totals.numbers["nonzero_flag_padding"] += flag_padding; totals.numbers["nonzero_function_padding"] += function_padding;
    totals.numbers["uninterpreted_low_flags"] += (flags & 0x1a) != 0;
    totals.numbers["unknown_comparison_operators"] += (flags >> 5) > 5;
    totals.numbers["absent_run_on_words"] += !run.has_value(); totals.numbers["absent_reference_words"] += !reference.has_value();
    totals.numbers["active_reference_words"] += selector && reference.has_value();
    totals.numbers["unverified_subject_selector_words"] += !group && run && *run > 4;
    if (run) ++(group ? totals.groups : totals.subjects)[*run];
    const char* operators[] = {"equal", "not_equal", "greater", "greater_or_equal", "less", "less_or_equal"};
    const auto comparison_operator = (flags >> 5) < 6 ? quote_json(operators[flags >> 5])
        : object({{"unknown", std::to_string(flags >> 5)}});
    return object({{"record_kind", quote_json(kind)}, {"form_id", std::to_string(form)}, {"record_file_offset", std::to_string(file_offset)},
        {"field_decoded_offset", std::to_string(field_offset)}, {"preceding_field_kind", previous ? quote_json(previous->first) : "null"},
        {"preceding_field_decoded_offset", previous ? std::to_string(previous->second) : "null"}, {"bytes", std::to_string(data.size())},
        {"sha256", quote_json(digest(data))}, {"flags", std::to_string(flags)},
        {"flag_padding", array({std::to_string(data[1]), std::to_string(data[2]), std::to_string(data[3])})},
        {"comparison_operator", comparison_operator}, {"comparison_value", object({{global ? "global_raw_form" : "float_bits", std::to_string(comparison)}})},
        {"function_id", std::to_string(function)}, {"function_padding", array({std::to_string(data[10]), std::to_string(data[11])})},
        {"parameter_words", array({std::to_string(integer(data, 12, 4)), std::to_string(integer(data, 16, 4))})},
        {"run_on_word", run ? std::to_string(*run) : "null"}, {"run_on_domain", quote_json(group ? "animation_group" : "subject_selection")},
        {"reference_word", reference ? std::to_string(*reference) : "null"}, {"reference_is_subject_selector", selector ? "true" : "false"}});
}

int wmain(int argc, wchar_t** argv) {
    try {
        if (argc != 3) throw std::runtime_error("usage: condition-oracle decoded-record-bundle FalloutNV.exe");
        const SourceImage source(argv[2]);
        std::ifstream input(argv[1], std::ios::binary | std::ios::ate); const auto size = input.tellg();
        if (!input || size < 8 || size > 256 * 1024 * 1024) throw std::runtime_error("bundle byte budget");
        Bytes bundle(static_cast<size_t>(size)); input.seekg(0); input.read(reinterpret_cast<char*>(bundle.data()), size);
        if (!input || std::string(bundle.begin(), bundle.begin() + 8) != "FRCOND01") throw std::runtime_error("bundle magic/read");
        std::vector<std::string> rows; Counts totals; size_t records = 0;
        for (size_t cursor = 8; cursor < bundle.size();) {
            if (++records > 262144) throw std::runtime_error("record budget");
            const auto kind_bytes = take(bundle, cursor, 4); const std::string kind(kind_bytes.begin(), kind_bytes.end());
            if (!std::all_of(kind.begin(), kind.end(), [](char c) { return (c >= 'A' && c <= 'Z') || c == '_'; })) throw std::runtime_error("record kind");
            const auto form = static_cast<uint32_t>(integer(bundle, cursor, 4)); cursor += 4;
            const auto offset = integer(bundle, cursor, 8); cursor += 8;
            const auto length = static_cast<size_t>(integer(bundle, cursor, 4)); cursor += 4;
            if (length > 64 * 1024 * 1024) throw std::runtime_error("record byte budget");
            const auto payload = take(bundle, cursor, length); const size_t before = rows.size();
            size_t fields = 0; std::optional<size_t> extended;
            std::optional<std::pair<std::string, size_t>> previous;
            for (size_t at = 0; at < payload.size();) {
                if (++fields > 1048576) throw std::runtime_error("field budget");
                const size_t field_offset = at; const auto signature = take(payload, at, 4);
                const std::string field(signature.begin(), signature.end());
                const auto short_size = static_cast<size_t>(integer(payload, at, 2)); at += 2;
                if (field == "XXXX") {
                    if (extended || short_size != 4) throw std::runtime_error("XXXX shape");
                    extended = static_cast<size_t>(integer(payload, at, 4)); at += 4; continue;
                }
                const auto data = take(payload, at, extended.value_or(short_size)); extended.reset();
                if (field == "CTDA") {
                    if (rows.size() >= 1000000) throw std::runtime_error("condition row budget");
                    rows.push_back(condition(data, kind, form, offset, field_offset, previous, totals));
                }
                previous = std::make_pair(field, field_offset);
            }
            if (extended) throw std::runtime_error("orphan XXXX");
            if (rows.size() == before) throw std::runtime_error("record contains no condition");
        }
        std::cout << object({{"schema_version", "1"}, {"bundle_sha256", quote_json(digest(bundle))},
            {"executable_source_sha256", quote_json(source.sha256)}, {"records", std::to_string(records)},
            {"rows", array(rows)}, {"counts", totals.json()}, {"evaluation_ready", "false"}}) << '\n';
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "condition-oracle: " << error.what() << '\n'; return 1;
    }
}
