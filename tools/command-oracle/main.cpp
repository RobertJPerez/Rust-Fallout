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

using Bytes = std::vector<unsigned char>;
using Object = std::map<std::string, std::string>;

static std::string quoted(const std::string& text) { std::ostringstream out; out << std::quoted(text); return out.str(); }
static std::string object(const Object& fields) {
    std::string out = "{"; bool comma = false;
    for (const auto& field : fields) { if (comma) out += ','; comma = true; out += quoted(field.first) + ':' + field.second; }
    return out + '}';
}
static std::string array(const std::vector<std::string>& rows) {
    std::string out = "["; bool comma = false;
    for (const auto& row : rows) { if (comma) out += ','; comma = true; out += row; }
    return out + ']';
}
static std::string digest(const Bytes& data) {
    BCRYPT_ALG_HANDLE algorithm = nullptr;
    if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) < 0) throw std::runtime_error("SHA256 provider");
    unsigned char hash[32]{};
    const auto status = BCryptHash(algorithm, nullptr, 0, const_cast<PUCHAR>(data.data()), static_cast<ULONG>(data.size()), hash, sizeof(hash));
    BCryptCloseAlgorithmProvider(algorithm, 0);
    if (status < 0) throw std::runtime_error("SHA256 failed");
    std::ostringstream out; out << std::hex << std::setfill('0');
    for (auto byte : hash) out << std::setw(2) << unsigned(byte);
    return out.str();
}

class SourceImage {
    Bytes data;
    IMAGE_OPTIONAL_HEADER32 optional{};
    std::vector<IMAGE_SECTION_HEADER> sections;
    template<class T> T read(size_t offset) const {
        if (offset > data.size() || sizeof(T) > data.size() - offset) throw std::runtime_error("PE field bounds");
        T value{}; std::memcpy(&value, data.data() + offset, sizeof(value)); return value;
    }
public:
    std::string sha256;
    size_t bytes() const { return data.size(); }
    uint32_t image_base() const { return optional.ImageBase; }
    uint32_t timestamp = 0;
    explicit SourceImage(const std::filesystem::path& path) {
        std::ifstream input(path, std::ios::binary | std::ios::ate);
        const auto size = input.tellg();
        if (!input || size < 64 || size > 64 * 1024 * 1024) throw std::runtime_error("source byte budget");
        data.resize(static_cast<size_t>(size)); input.seekg(0);
        input.read(reinterpret_cast<char*>(data.data()), size);
        if (!input) throw std::runtime_error("source read");
        sha256 = digest(data);
        if (sha256 != "3a87f92f011e5dc9179ddf733cf08be2b39ea6e5b7a8a9e3a9a72dafcc1b104d") throw std::runtime_error("unsupported executable fingerprint");
        const auto dos = read<IMAGE_DOS_HEADER>(0);
        if (dos.e_magic != IMAGE_DOS_SIGNATURE || dos.e_lfanew < 64) throw std::runtime_error("DOS header");
        const size_t pe = static_cast<size_t>(dos.e_lfanew);
        if (read<DWORD>(pe) != IMAGE_NT_SIGNATURE) throw std::runtime_error("PE signature");
        const auto file = read<IMAGE_FILE_HEADER>(pe + sizeof(DWORD));
        if (file.Machine != IMAGE_FILE_MACHINE_I386 || file.NumberOfSections == 0 || file.NumberOfSections > 96
            || file.SizeOfOptionalHeader < sizeof(optional)) throw std::runtime_error("PE source variant");
        optional = read<IMAGE_OPTIONAL_HEADER32>(pe + sizeof(DWORD) + sizeof(file));
        if (optional.Magic != IMAGE_NT_OPTIONAL_HDR32_MAGIC) throw std::runtime_error("PE optional header");
        timestamp = file.TimeDateStamp;
        const size_t table = pe + sizeof(DWORD) + sizeof(file) + file.SizeOfOptionalHeader;
        for (size_t i = 0; i < file.NumberOfSections; ++i) sections.push_back(read<IMAGE_SECTION_HEADER>(table + i * sizeof(IMAGE_SECTION_HEADER)));
    }
    size_t offset(uint32_t address, size_t count) const {
        if (address < optional.ImageBase) throw std::runtime_error("address below image");
        const uint32_t rva = address - optional.ImageBase;
        std::vector<size_t> matches;
        for (const auto& section : sections) {
            if (rva < section.VirtualAddress) continue;
            const size_t delta = size_t(rva) - section.VirtualAddress;
            if (delta >= section.SizeOfRawData || count > section.SizeOfRawData - delta) continue;
            const size_t at = size_t(section.PointerToRawData) + delta;
            if (at <= data.size() && count <= data.size() - at) matches.push_back(at);
        }
        if (matches.size() != 1) throw std::runtime_error("address lacks unique initialized extent");
        return matches[0];
    }
    uint32_t number(uint32_t address, size_t width) const {
        const size_t at = offset(address, width); uint32_t value = 0;
        for (size_t i = 0; i < width; ++i) value |= uint32_t(data.at(at + i)) << (i * 8);
        return value;
    }
    std::string name(uint32_t address) const {
        std::string value;
        for (size_t i = 0; i <= 128; ++i) {
            if (uint64_t(address) + i > UINT32_MAX) throw std::runtime_error("string address overflow");
            const auto byte = number(address + static_cast<uint32_t>(i), 1);
            if (byte == 0) return value;
            if (byte < 32 || byte > 126) throw std::runtime_error("unsupported name bytes");
            value += static_cast<char>(byte);
        }
        throw std::runtime_error("name byte budget");
    }
};

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
                {"type_name",quoted(source.name(source.number(param,4)))},
                {"type_id",std::to_string(source.number(param+4,4))},{"optional_word",std::to_string(source.number(param+8,4))}}));
        }
        const auto short_pointer = source.number(at + 4, 4);
        rows.push_back(object({{"descriptor_file_offset",std::to_string(source.offset(at,40))},
            {"table_index",std::to_string(index)},{"id",std::to_string(first_id+index)},
            {"stored_opcode",std::to_string(source.number(at+8,4))},{"name",quoted(source.name(source.number(at,4)))},
            {"short_name",short_pointer ? quoted(source.name(short_pointer)) : "null"},
            {"needs_parent_word",std::to_string(source.number(at+16,2))},{"parameters",array(parameters)},
            {"execute_handler_present",source.number(at+24,4) ? "true" : "false"},
            {"parse_handler_present",source.number(at+28,4) ? "true" : "false"},
            {"condition_handler_present",source.number(at+32,4) ? "true" : "false"},
            {"flags",std::to_string(source.number(at+36,4))},{"implementation_status",quoted("metadata-decoded; behavior unimplemented")}}));
    }
    return array(rows);
}

int wmain(int argc, wchar_t** argv) {
    try {
        if (argc != 2) throw std::runtime_error("usage: command-oracle FalloutNV.exe");
        const SourceImage source(argv[1]);
        const auto output = object({{"schema_version","1"},{"source_sha256",quoted(source.sha256)},
            {"source_bytes",std::to_string(source.bytes())},{"image_base",std::to_string(source.image_base())},
            {"pe_timestamp",std::to_string(source.timestamp)},
            {"script_commands",descriptors(source,0x01190910,640,0x1000)},
            {"event_blocks",descriptors(source,0x0118e2f0,38,0)},
            {"statements",descriptors(source,0x0118cb50,16,0x10)},
            {"execution_ready","false"},{"retail_parity_accepted","false"}});
        std::cout << output << '\n'; return 0;
    } catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
}
