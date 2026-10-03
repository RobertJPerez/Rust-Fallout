// Offline header comparison, authored independently of the Rust decoder.
// xNVSE 0ccd23ad documents the 4/8-byte line headers and six-byte begin prefix.
// Input bodies were extracted by Rust: plugin framing, decompression, script
// binding and execution are outside this tool's comparison scope.
#include <windows.h>
#include <bcrypt.h>
#include <algorithm>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <map>
#include <sstream>
#include <stdexcept>
#include <vector>

using Bytes = std::vector<unsigned char>;

static uint32_t read_le(const Bytes& data, size_t at, size_t width, size_t end) {
    if (at > end || width > end - at) throw std::runtime_error("truncated field");
    uint32_t value = 0;
    for (size_t i = 0; i < width; ++i) value |= uint32_t(data.at(at + i)) << (8 * i);
    return value;
}

static void append_le(Bytes& data, uint32_t value, size_t width) {
    for (size_t i = 0; i < width; ++i) data.push_back(static_cast<unsigned char>(value >> (8 * i)));
}

static std::string digest(const Bytes& data) {
    BCRYPT_ALG_HANDLE algorithm = nullptr;
    if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) < 0)
        throw std::runtime_error("SHA256 provider unavailable");
    unsigned char hash[32]{};
    const auto status = BCryptHash(algorithm, nullptr, 0, const_cast<PUCHAR>(data.data()),
        static_cast<ULONG>(data.size()), hash, sizeof(hash));
    BCryptCloseAlgorithmProvider(algorithm, 0);
    if (status < 0) throw std::runtime_error("SHA256 failed");
    std::ostringstream out; out << std::hex << std::setfill('0');
    for (auto byte : hash) out << std::setw(2) << unsigned(byte);
    return out.str();
}

struct Framing {
    size_t instructions = 0, reference_calls = 0, event_blocks = 0;
    Bytes tuples;
    std::map<uint16_t, uint64_t> opcodes, events;
};

static Framing frame(const Bytes& data) {
    if (data.size() > 4 * 1024 * 1024) throw std::runtime_error("body exceeds byte budget");
    Framing result;
    for (size_t line = 0; line < data.size();) {
        if (result.instructions >= 262144) throw std::runtime_error("instruction budget");
        const bool reference = read_le(data, line, 2, data.size()) == 0x1c;
        const size_t header = reference ? 8 : 4;
        if (data.size() - line < header) throw std::runtime_error("partial instruction header");
        const auto opcode = read_le(data, line + (reference ? 4 : 0), 2, data.size());
        const auto length = read_le(data, line + header - 2, 2, data.size());
        const size_t payload = line + header;
        if (length > data.size() - payload) throw std::runtime_error("operand extent exceeds body");
        const size_t next = payload + length;
        const bool event = opcode == 0x10;
        if (event && length < 6) throw std::runtime_error("short begin event header");
        const auto caller = reference ? read_le(data, line + 2, 2, payload) : 0;
        const auto event_id = event ? read_le(data, payload, 2, next) : 0;
        const auto jump = event ? read_le(data, payload + 2, 4, next) : 0;
        append_le(result.tuples, static_cast<uint32_t>(line), 4);
        append_le(result.tuples, static_cast<uint32_t>(next), 4);
        append_le(result.tuples, static_cast<uint32_t>(payload), 4);
        append_le(result.tuples, opcode, 2);
        append_le(result.tuples, reference ? 1 : 0, 1);
        append_le(result.tuples, caller, 2);
        append_le(result.tuples, event ? 1 : 0, 1);
        append_le(result.tuples, event_id, 2);
        append_le(result.tuples, jump, 4);
        ++result.instructions;
        ++result.opcodes[static_cast<uint16_t>(opcode)];
        if (reference) ++result.reference_calls;
        if (event) { ++result.event_blocks; ++result.events[static_cast<uint16_t>(event_id)]; }
        line = next;
    }
    return result;
}

static std::string counts(const std::map<uint16_t, uint64_t>& values) {
    std::ostringstream out; out << '{'; bool comma = false;
    for (const auto& entry : values) {
        if (comma) out << ','; comma = true;
        out << '"' << entry.first << "\":" << entry.second;
    }
    out << '}'; return out.str();
}

static void compare(const std::filesystem::path& path) {
    std::ifstream input(path, std::ios::binary | std::ios::ate);
    const auto size = input.tellg();
    if (!input || size < 8 || size > 66 * 1024 * 1024) throw std::runtime_error("bundle byte budget");
    Bytes bundle(static_cast<size_t>(size)); input.seekg(0);
    input.read(reinterpret_cast<char*>(bundle.data()), size);
    if (!input) throw std::runtime_error("bundle read failed");
    const Bytes magic{'F','R','O','B','S','0','0','1'};
    if (!std::equal(magic.begin(), magic.end(), bundle.begin())) throw std::runtime_error("bundle magic");
    std::ostringstream bodies; bodies << '['; size_t count = 0;
    for (size_t at = 8; at < bundle.size();) {
        if (count >= 262144) throw std::runtime_error("bundle body budget");
        const auto length = read_le(bundle, at, 4, bundle.size()); at += 4;
        if (length > 4 * 1024 * 1024 || length > bundle.size() - at) throw std::runtime_error("body extent");
        const Bytes data(bundle.begin() + at, bundle.begin() + at + length); at += length;
        const auto decoded = frame(data);
        if (count++) bodies << ',';
        bodies << "{\"bytes\":" << data.size() << ",\"sha256\":\"" << digest(data)
            << "\",\"framing_sha256\":\"" << digest(decoded.tuples)
            << "\",\"instructions\":" << decoded.instructions
            << ",\"reference_calls\":" << decoded.reference_calls
            << ",\"event_blocks\":" << decoded.event_blocks
            << ",\"instruction_opcodes\":" << counts(decoded.opcodes)
            << ",\"event_ids\":" << counts(decoded.events) << '}';
    }
    bodies << ']';
    std::cout << "{\"schema_version\":1,\"bundle_sha256\":\"" << digest(bundle)
        << "\",\"compiled_bodies\":" << count << ",\"execution_ready\":false,\"bodies\":" << bodies.str() << "}\n";
}

int wmain(int argc, wchar_t** argv) {
    try {
        if (argc != 2) throw std::runtime_error("usage: script-oracle comparison-bundle");
        compare(argv[1]); return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n'; return 1;
    }
}
