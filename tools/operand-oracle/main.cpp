// Original offline operand association comparison.
#include "../oracle-common/operand_binding_source.hpp"

int wmain(int argc, wchar_t** argv) {
    try {
        if (argc != 3) throw std::runtime_error("usage: operand-oracle decoded-record-bundle FalloutNV.exe");
        setlocale(LC_NUMERIC, "C"); const SourceImage source(argv[2]);
        std::vector<std::string> operator_rows; const auto ops = operators(source, operator_rows);
        const auto catalogue = signatures(source);
        std::ifstream input(argv[1], std::ios::binary | std::ios::ate); const auto size = input.tellg();
        if (!input || size < 8 || size > 512 * 1024 * 1024) throw std::runtime_error("bundle byte budget");
        Bytes bundle(static_cast<size_t>(size)); input.seekg(0); input.read(reinterpret_cast<char*>(bundle.data()), size);
        if (!input || std::string(bundle.begin(), bundle.begin() + 8) != "FRUNIT01") throw std::runtime_error("bundle magic/read");
        size_t records = 0; uint64_t uses = 0;
        std::vector<std::string> rows;
        for (size_t at = 8; at < bundle.size();) {
            if (++records > 262144) throw std::runtime_error("record budget");
            const auto signature_bytes = take(bundle, at, 4);
            const std::string kind(signature_bytes.begin(), signature_bytes.end());
            if (!std::all_of(kind.begin(), kind.end(), [](char c) { return (c >= 'A' && c <= 'Z') || c == '_'; }))
                throw std::runtime_error("record signature");
            const auto form = number(bundle, at, 4); at += 4;
            const auto file_offset = fallout_tables::integer(bundle, at, 8); at += 8;
            const auto length = number(bundle, at, 4); at += 4;
            if (length > 64 * 1024 * 1024) throw std::runtime_error("record byte budget");
            const auto scripts = fallout_tables::units(take(bundle, at, length));
            if (scripts.empty()) throw std::runtime_error("record lacks script unit");
            for (const auto& fields : scripts) {
                const auto unit = fallout_tables::inspect(fields);
                if (!unit.compiled) continue;
                if (rows.size() >= 65536) throw std::runtime_error("compiled unit budget");
                const auto binding = bind(unit, ops, catalogue); uses += binding.uses;
                if (uses > 2000000) throw std::runtime_error("run use budget");
                rows.push_back(object({{"record_kind", quote_json(kind)}, {"form_id", std::to_string(form)},
                    {"record_file_offset", std::to_string(file_offset)}, {"header_decoded_offset", std::to_string(fields.front().offset)},
                    {"metadata_sha256", quote_json(digest(unit.metadata))}, {"compiled_bytes", std::to_string(unit.compiled->size())},
                    {"compiled_sha256", quote_json(digest(*unit.compiled))}, {"binding_sha256", quote_json(digest(binding.tuples))},
                    {"counts", binding.counts_json()}, {"decode_issues", "[]"}, {"missing_bindings", "[]"}}));
            }
        }
        std::cout << object({{"schema_version", "1"}, {"bundle_sha256", quote_json(digest(bundle))},
            {"executable_source_sha256", quote_json(source.sha256)}, {"compiled_units", array(rows)},
            {"compiled_unit_count", std::to_string(rows.size())}, {"execution_ready", "false"}}) << '\n';
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "operand-oracle: " << error.what() << '\n'; return 1;
    }
}
