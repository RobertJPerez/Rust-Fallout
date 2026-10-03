// Shared helpers for original offline comparison tools. Runtime crates never
// include this file. Source addresses map initialized file bytes only.
#pragma once
#include <windows.h>
#include <bcrypt.h>
#include <algorithm>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>
namespace fallout_oracle {
using Bytes = std::vector<unsigned char>;
inline std::string digest(const Bytes& data) {
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

} // namespace fallout_oracle
