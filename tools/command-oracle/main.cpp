// Original offline descriptor comparison. Unlike the script-header oracle, this
// reads the executable directly and maps its PE sections independently of Rust.
// It neither loads the image nor invokes its handlers. Locations and structures
// are documented by xNVSE 0ccd23ad, not inferred from a guessed runtime version.
#include <windows.h>
#include <bcrypt.h>
#include <algorithm>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <map>
#include <sstream>
#include <stdexcept>
#include <vector>

#include "../oracle-common/pe_source.hpp"
using fallout_oracle::SourceImage;
using fallout_oracle::digest;
using Bytes = std::vector<unsigned char>;
using Object = std::map<std::string, std::string>;

static std::string json_quote(const std::string& text) { std::ostringstream out; out << std::quoted(text); return out.str(); }
static std::string object(const Object& fields) {
    std::string out = "{"; bool comma = false;
    for (const auto& field : fields) { if (comma) out += ','; comma = true; out += json_quote(field.first) + ':' + field.second; }
    return out + '}';
}
static std::string array(const std::vector<std::string>& rows) {
    std::string out = "["; bool comma = false;
    for (const auto& row : rows) { if (comma) out += ','; comma = true; out += row; }
    return out + ']';
}

static std::string descriptors(const SourceImage& source, uint32_t start, size_t count, uint32_t first_id) {
    std::vector<std::string> rows;
    for (size_t index = 0; index < count; ++index) {
        const auto at = start + static_cast<uint32_t>(index) * 40;
        const auto parameter_count = source.number(at + 18, 2);
        if (parameter_count > 64) throw std::runtime_error("parameter budget");
        std::vector<std::string> parameters;
        for (uint32_t i = 0; i < parameter_count; ++i) {
            const uint64_t pointer = uint64_t(source.number(at + 20, 4)) + i * 12;
            if (pointer > UINT32_MAX) throw std::runtime_error("parameter address overflow");
            const auto param = static_cast<uint32_t>(pointer);
            parameters.push_back(object({{"descriptor_file_offset",std::to_string(source.offset(param,12))},
                {"type_name",json_quote(source.name(source.number(param,4)))},
                {"type_id",std::to_string(source.number(param+4,4))},{"optional_word",std::to_string(source.number(param+8,4))}}));
        }
        const auto short_pointer = source.number(at + 4, 4);
        rows.push_back(object({{"descriptor_file_offset",std::to_string(source.offset(at,40))},
            {"table_index",std::to_string(index)},{"id",std::to_string(first_id+index)},
            {"stored_opcode",std::to_string(source.number(at+8,4))},{"name",json_quote(source.name(source.number(at,4)))},
            {"short_name",short_pointer ? json_quote(source.name(short_pointer)) : "null"},
            {"needs_parent_word",std::to_string(source.number(at+16,2))},{"parameters",array(parameters)},
            {"execute_handler_present",source.number(at+24,4) ? "true" : "false"},
            {"parse_handler_present",source.number(at+28,4) ? "true" : "false"},
            {"parse_convention",json_quote(source.number(at+28,4)==0x005b1ba0 ? "vanilla-default" :
                (source.number(at+28,4)==0x005b3c70 || source.number(at+28,4)==0x005b3ca0 ||
                 source.number(at+28,4)==0x005b3c40 || source.number(at+28,4)==0x005b3cd0) ? "vanilla-message" : "unverified")},
            {"condition_handler_present",source.number(at+32,4) ? "true" : "false"},
            {"flags",std::to_string(source.number(at+36,4))},{"implementation_status",json_quote("metadata-decoded; behavior unimplemented")}}));
    }
    return array(rows);
}

int wmain(int argc, wchar_t** argv) {
    try {
        if (argc != 2) throw std::runtime_error("usage: command-oracle FalloutNV.exe");
        const SourceImage source(argv[1]);
        std::vector<std::string> operators;
        for (uint32_t index = 0; index < 16; ++index) {
            const uint32_t at = 0x0118cad0 + index * 8;
            std::string spelling;
            std::vector<std::string> bytes;
            bool terminated = false;
            for (uint32_t i = 0; i < 3; ++i) {
                const auto byte = source.number(at + 5 + i, 1);
                bytes.push_back(std::to_string(byte));
                if (!byte) terminated = true;
                else if (!terminated) spelling += static_cast<char>(byte);
            }
            if (!terminated || spelling.empty()) throw std::runtime_error("operator spelling");
            operators.push_back(object({{"descriptor_file_offset",std::to_string(source.offset(at,8))},
                {"table_index",std::to_string(index)},{"code",std::to_string(source.number(at,4))},
                {"precedence",std::to_string(source.number(at+4,1))},{"spelling",json_quote(spelling)},
                {"raw_spelling_bytes",array(bytes)}}));
        }
        const auto output = object({{"schema_version","1"},{"source_sha256",json_quote(source.sha256)},
            {"source_bytes",std::to_string(source.bytes())},{"image_base",std::to_string(source.image_base())},
            {"pe_timestamp",std::to_string(source.timestamp)},
            {"script_commands",descriptors(source,0x01190910,640,0x1000)},
            {"event_blocks",descriptors(source,0x0118e2f0,38,0)},
            {"statements",descriptors(source,0x0118cb50,16,0x10)},
            {"operators",array(operators)},
            {"execution_ready","false"},{"retail_parity_accepted","false"}});
        std::cout << output << '\n'; return 0;
    } catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
}
