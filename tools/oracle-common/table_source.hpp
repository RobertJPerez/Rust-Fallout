// Original bounded decoded-record script table reader.
#pragma once
#include <windows.h>
#include <bcrypt.h>
#include <algorithm>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <map>
#include <optional>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>


namespace fallout_tables {
using Bytes = std::vector<unsigned char>;
static uint64_t integer(const Bytes& data, size_t at, size_t width) {
    if (at > data.size() || width > data.size() - at || width > 8)
        throw std::runtime_error("truncated integer");
    uint64_t result = 0;
    for (size_t i = 0; i < width; ++i) result |= uint64_t(data[at + i]) << (i * 8);
    return result;
}
static void append(Bytes& data, uint64_t value, size_t width) {
    for (size_t i = 0; i < width; ++i) data.push_back(static_cast<unsigned char>(value >> (i * 8)));
}
static std::string hash(const Bytes& data) {
    BCRYPT_ALG_HANDLE provider = nullptr;
    if (BCryptOpenAlgorithmProvider(&provider, BCRYPT_SHA256_ALGORITHM, nullptr, 0) < 0)
        throw std::runtime_error("SHA256 provider");
    unsigned char digest[32]{};
    auto status = BCryptHash(provider, nullptr, 0, const_cast<PUCHAR>(data.data()),
        static_cast<ULONG>(data.size()), digest, sizeof(digest));
    BCryptCloseAlgorithmProvider(provider, 0);
    if (status < 0) throw std::runtime_error("SHA256 hash");
    std::ostringstream out; out << std::hex << std::setfill('0');
    for (auto byte : digest) out << std::setw(2) << unsigned(byte);
    return out.str();
}
struct Field { std::string kind; size_t offset; Bytes data; };
static bool selected(const std::string& kind) {
    return kind == "SCHR" || kind == "SCDA" || kind == "SCTX" || kind == "SLSD"
        || kind == "SCVR" || kind == "SCRO" || kind == "SCRV";
}
static std::vector<std::vector<Field>> units(const Bytes& payload) {
    std::vector<std::vector<Field>> result;
    std::optional<size_t> extended;
    size_t field_count = 0;
    for (size_t cursor = 0; cursor < payload.size();) {
        if (++field_count > 1048576 || payload.size() - cursor < 6)
            throw std::runtime_error("record field budget or header");
        const size_t offset = cursor;
        const std::string kind(payload.begin() + cursor, payload.begin() + cursor + 4);
        const size_t short_size = static_cast<size_t>(integer(payload, cursor + 4, 2));
        cursor += 6;
        if (kind == "XXXX") {
            if (extended || short_size != 4) throw std::runtime_error("invalid XXXX");
            extended = static_cast<size_t>(integer(payload, cursor, 4)); cursor += 4;
            continue;
        }
        const size_t size = extended.value_or(short_size); extended.reset();
        if (size > payload.size() - cursor) throw std::runtime_error("record field extent");
        if (selected(kind)) {
            if (kind == "SCHR") {
                if (size != 20 || result.size() >= 65536) throw std::runtime_error("SCHR shape or budget");
                result.emplace_back();
            }
            if (result.empty()) throw std::runtime_error("unowned script field");
            result.back().push_back({kind, offset, Bytes(payload.begin() + cursor, payload.begin() + cursor + size)});
        }
        cursor += size;
    }
    if (extended) throw std::runtime_error("orphan XXXX");
    return result;
}
struct Variable { Bytes declaration, name; size_t offset; };
struct TableEntry { uint8_t kind; uint32_t target; size_t offset; };
struct Summary {
    Bytes metadata, calls;
    std::map<uint32_t, Variable> variables;
    std::vector<TableEntry> references;
    std::optional<Bytes> compiled;
    std::vector<std::string> issues;
    size_t variable_count = 0, duplicate_variables = 0, conflicting_variables = 0;
    size_t reference_calls = 0, calls_forms = 0, calls_variables = 0;
};
static Summary inspect(const std::vector<Field>& fields) {
    Summary result;
    std::optional<Bytes> pending;
    std::optional<size_t> pending_offset;
    bool source = false;
    for (const auto& field : fields) {
        result.metadata.insert(result.metadata.end(), field.kind.begin(), field.kind.end());
        append(result.metadata, field.offset, 4); append(result.metadata, field.data.size(), 4);
        result.metadata.insert(result.metadata.end(), field.data.begin(), field.data.end());
        if (pending && field.kind != "SCVR") throw std::runtime_error("unnamed SLSD");
        if (field.kind == "SCDA") {
            if (result.compiled) throw std::runtime_error("duplicate SCDA");
            result.compiled = field.data;
        } else if (field.kind == "SCTX") {
            if (source) throw std::runtime_error("duplicate SCTX"); source = true;
        } else if (field.kind == "SLSD") {
            if (field.data.size() != 24 || result.variable_count >= 65536) throw std::runtime_error("SLSD extent or budget");
            pending = field.data; pending_offset = field.offset;
        } else if (field.kind == "SCVR") {
            if (!pending || field.data.empty() || field.data.back() != 0
                || std::find(field.data.begin(), field.data.end() - 1, 0) != field.data.end() - 1)
                throw std::runtime_error("SCVR pairing or termination");
            const auto index = static_cast<uint32_t>(integer(*pending, 0, 4));
            const auto existing = result.variables.find(index);
            if (existing != result.variables.end()) {
                ++result.duplicate_variables;
                if (existing->second.declaration != *pending || existing->second.name != field.data)
                    ++result.conflicting_variables;
            } else result.variables.emplace(index, Variable{*pending, field.data, *pending_offset});
            ++result.variable_count; pending.reset();
        } else if (field.kind == "SCRO" || field.kind == "SCRV") {
            if (field.data.size() != 4 || result.references.size() >= 65536) throw std::runtime_error("reference extent or budget");
            result.references.push_back({static_cast<uint8_t>(field.kind == "SCRV"), static_cast<uint32_t>(integer(field.data, 0, 4)), field.offset});
        }
    }
    if (pending) throw std::runtime_error("unnamed final SLSD");
    const auto& header = fields.front().data;
    if (integer(header, 4, 4) != result.references.size())
        result.issues.push_back("SCHR reference count differs from ordered table length");
    if (integer(header, 8, 4) != (result.compiled ? result.compiled->size() : 0))
        result.issues.push_back("SCHR compiled size differs from SCDA extent");
    for (const auto& reference : result.references)
        if (reference.kind && !result.variables.count(reference.target)) throw std::runtime_error("SCRV has no declaration");
    if (result.compiled) {
        const auto& data = *result.compiled;
        if (data.size() > 4 * 1024 * 1024) throw std::runtime_error("compiled byte budget");
        size_t instructions = 0;
        for (size_t cursor = 0; cursor < data.size();) {
            if (++instructions > 262144) throw std::runtime_error("instruction budget");
            const size_t start = cursor;
            const bool reference = integer(data, cursor, 2) == 0x1c;
            const size_t header_size = reference ? 8 : 4;
            const auto length = static_cast<size_t>(integer(data, cursor + header_size - 2, 2));
            const auto opcode = integer(data, cursor + (reference ? 4 : 0), 2);
            cursor += header_size;
            if (length > data.size() - cursor) throw std::runtime_error("instruction operand extent");
            if (opcode == 0x10 && length < 6) throw std::runtime_error("short begin header");
            if (reference) {
                const auto index = static_cast<uint16_t>(integer(data, start + 2, 2));
                if (!index || index > result.references.size()) throw std::runtime_error("caller outside one-based table");
                const auto& target = result.references[index - 1];
                ++result.reference_calls;
                if (target.kind) ++result.calls_variables; else ++result.calls_forms;
                append(result.calls, start, 4); append(result.calls, index, 2);
                append(result.calls, target.kind, 1); append(result.calls, target.target, 4);
            }
            cursor += length;
        }
    }
    return result;
}
}
