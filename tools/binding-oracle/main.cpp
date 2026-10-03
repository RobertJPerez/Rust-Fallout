// Original offline comparison of authored script tables and caller associations.
// Starts at Rust-extracted decoded records. Plugin extraction/decompression,
// loaded form existence, variable values and execution are outside its scope.
// Layout references: pinned xEdit FNV schema and xNVSE GameScript lookups.
#include "../oracle-common/table_source.hpp"
using namespace fallout_tables;

static void compare(const std::filesystem::path& path) {
    std::ifstream input(path, std::ios::binary | std::ios::ate);
    const auto size = input.tellg();
    if (!input || size < 8 || size > 512 * 1024 * 1024) throw std::runtime_error("bundle byte budget");
    Bytes bundle(static_cast<size_t>(size)); input.seekg(0); input.read(reinterpret_cast<char*>(bundle.data()), size);
    if (!input || std::string(bundle.begin(), bundle.begin() + 8) != "FRUNIT01") throw std::runtime_error("bundle read or magic");
    std::ostringstream rows; rows << '['; size_t count = 0, records = 0;
    for (size_t cursor = 8; cursor < bundle.size();) {
        if (bundle.size() - cursor < 20 || ++records > 262144) throw std::runtime_error("bundle record header or budget");
        const std::string kind(bundle.begin() + cursor, bundle.begin() + cursor + 4);
        if (kind.size() != 4 || !std::all_of(kind.begin(), kind.end(), [](char c) { return (c >= 'A' && c <= 'Z') || c == '_'; }))
            throw std::runtime_error("record signature");
        const auto form = integer(bundle, cursor + 4, 4), offset = integer(bundle, cursor + 8, 8);
        const auto length = static_cast<size_t>(integer(bundle, cursor + 16, 4)); cursor += 20;
        if (length > 64 * 1024 * 1024 || length > bundle.size() - cursor) throw std::runtime_error("bundle record extent");
        const Bytes payload(bundle.begin() + cursor, bundle.begin() + cursor + length); cursor += length;
        const auto scripts = units(payload);
        if (scripts.empty()) throw std::runtime_error("bundle record contains no script");
        for (const auto& fields : scripts) {
            if (count >= 262144) throw std::runtime_error("bundle unit budget");
            const auto summary = inspect(fields);
            if (count++) rows << ',';
            rows << "{\"record_kind\":\"" << kind << "\",\"form_id\":" << form
                << ",\"record_file_offset\":" << offset << ",\"header_decoded_offset\":" << fields.front().offset
                << ",\"script_fields\":" << fields.size() << ",\"metadata_sha256\":\"" << hash(summary.metadata)
                << "\",\"declared_references\":" << integer(fields.front().data, 4, 4)
                << ",\"declared_compiled_bytes\":" << integer(fields.front().data, 8, 4)
                << ",\"declared_variables\":" << integer(fields.front().data, 12, 4)
                << ",\"compiled_bytes\":" << (summary.compiled ? std::to_string(summary.compiled->size()) : "null")
                << ",\"variables\":" << summary.variable_count
                << ",\"duplicate_variable_indices\":" << summary.duplicate_variables
                << ",\"conflicting_variable_indices\":" << summary.conflicting_variables
                << ",\"max_variable_index\":" << (summary.variables.empty() ? "null" : std::to_string(summary.variables.rbegin()->first))
                << ",\"references\":" << summary.references.size()
                << ",\"reference_calls\":" << summary.reference_calls
                << ",\"calls_to_forms\":" << summary.calls_forms << ",\"calls_to_variables\":" << summary.calls_variables
                << ",\"caller_bindings_sha256\":\"" << hash(summary.calls) << "\",\"issues\":[";
            for (size_t i = 0; i < summary.issues.size(); ++i) {
                if (i) rows << ',';
                rows << '"' << summary.issues[i] << '"';
            }
            rows << "]}";
        }
    }
    rows << ']';
    std::cout << "{\"schema_version\":1,\"bundle_sha256\":\"" << hash(bundle)
        << "\",\"records\":" << records << ",\"unit_count\":" << count
        << ",\"execution_ready\":false,\"units\":" << rows.str() << "}\n";
}
int wmain(int argc, wchar_t** argv) {
    try {
        if (argc != 2) throw std::runtime_error("usage: binding-oracle raw-record-bundle");
        compare(argv[1]); return 0;
    } catch (const std::exception& error) {
        std::cerr << "binding-oracle: " << error.what() << '\n'; return 1;
    }
}
