// Original direct plugin-header reader shared by offline comparison tools.
#pragma once
#include "table_source.hpp"
#include <memory>
#include <set>
#include <tuple>
#include <cstring>

namespace fallout_records {
using fallout_tables::Bytes;
using fallout_tables::integer;
using fallout_tables::append;
using Object = std::map<std::string, std::string>;
using Word = std::optional<uint32_t>;
static std::string quote(const std::string& value) { std::ostringstream out; out << std::quoted(value); return out.str(); }
static std::string object(const Object& values) {
    std::string out = "{"; bool comma = false;
    for (const auto& value : values) { if (comma) out += ','; comma = true; out += quote(value.first) + ':' + value.second; } return out + '}';
}
static std::string array(const std::vector<std::string>& values) {
    std::string out = "["; bool comma = false; for (const auto& value : values) { if (comma) out += ','; comma = true; out += value; } return out + ']';
}
static std::string lower(std::string value) {
    if (value.empty() || value == "." || value == ".." || value.size() > 4096) throw std::runtime_error("plugin name");
    for (auto& c : value) {
        if (static_cast<unsigned char>(c) < 32 || static_cast<unsigned char>(c) >= 128 || c == '/' || c == '\\' || c == ':') throw std::runtime_error("plugin name byte");
        if (c >= 'A' && c <= 'Z') c += 'a' - 'A';
    } return value;
}

class Hash {
    BCRYPT_ALG_HANDLE provider = nullptr; BCRYPT_HASH_HANDLE handle = nullptr;
public:
    Hash(const Hash&) = delete;
    Hash& operator=(const Hash&) = delete;
    Hash() {
        if (BCryptOpenAlgorithmProvider(&provider, BCRYPT_SHA256_ALGORITHM, nullptr, 0) < 0) throw std::runtime_error("hash provider");
        if (BCryptCreateHash(provider, &handle, nullptr, 0, nullptr, 0, 0) < 0) { BCryptCloseAlgorithmProvider(provider, 0); throw std::runtime_error("hash creation"); }
    }
    ~Hash() { BCryptDestroyHash(handle); BCryptCloseAlgorithmProvider(provider, 0); }
    void update(const Bytes& bytes) { if (BCryptHashData(handle, const_cast<PUCHAR>(bytes.data()), static_cast<ULONG>(bytes.size()), 0) < 0) throw std::runtime_error("hash update"); }
    std::string finish() {
        unsigned char bytes[32]{}; if (BCryptFinishHash(handle, bytes, sizeof(bytes), 0) < 0) throw std::runtime_error("hash finish");
        std::ostringstream out; out << std::hex << std::setfill('0'); for (auto byte : bytes) out << std::setw(2) << unsigned(byte); return out.str();
    }
};
struct Source {
    HANDLE guard = INVALID_HANDLE_VALUE; std::ifstream input; uint64_t bytes = 0; std::string sha256;
    explicit Source(const std::filesystem::path& path) {
        guard = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
        if (guard == INVALID_HANDLE_VALUE) throw std::runtime_error("source read lock");
        try {
            LARGE_INTEGER size{}; if (!GetFileSizeEx(guard, &size) || size.QuadPart < 24) throw std::runtime_error("source size"); bytes = size.QuadPart;
            input.open(path, std::ios::binary); if (!input) throw std::runtime_error("source stream");
            Hash hash; Bytes buffer(65536); uint64_t remaining = bytes;
            while (remaining) { const auto size = static_cast<size_t>(std::min<uint64_t>(remaining, buffer.size())); buffer.resize(size);
                input.read(reinterpret_cast<char*>(buffer.data()), size); if (!input) throw std::runtime_error("source hash read"); hash.update(buffer); remaining -= size; }
            sha256 = hash.finish(); input.clear(); input.seekg(0);
        } catch (...) { CloseHandle(guard); guard = INVALID_HANDLE_VALUE; throw; }
    }
    ~Source() { if (guard != INVALID_HANDLE_VALUE) CloseHandle(guard); }
    Bytes read(uint64_t offset, size_t size) {
        if (offset > bytes || size > bytes - offset) throw std::runtime_error("source extent");
        input.seekg(offset); Bytes result(size); input.read(reinterpret_cast<char*>(result.data()), size);
        if (!input) throw std::runtime_error("source exact read"); return result;
    }
};
struct Key {
    std::string origin; uint32_t local;
    bool operator<(const Key& other) const { return std::tie(origin, local) < std::tie(other.origin, other.local); }
    std::string json() const { return object({{"profile", quote("nv-original")}, {"origin_plugin", quote(origin)}, {"local_id", std::to_string(local)}}); }
};
static std::optional<Key> resolve(const std::string& name, const std::vector<std::string>& masters, uint32_t raw) {
    if (!raw) return {};
    const size_t selector = raw >> 24; return Key{lower(selector < masters.size() ? masters[selector] : name), raw & 0xffffff};
}
struct Entry {
    size_t source; uint64_t offset; Bytes header; Word topic, world, cell, child;
};
struct Group { uint64_t end; uint32_t label; int32_t kind; };
struct Plugin { std::string name; std::vector<std::string> masters; std::unique_ptr<Source> source; uint64_t definitions = 0; };
static void text(Bytes& out, const std::string& value) {
    if (value.size() > UINT16_MAX) throw std::runtime_error("metadata name budget"); append(out, value.size(), 2); out.insert(out.end(), value.begin(), value.end());
}
static void option(Bytes& out, Word value) { append(out, value.has_value(), 1); append(out, value.value_or(0), 4); }
static void metadata(Hash& hash, const Key& key, const std::string& source, const Entry& entry) {
    Bytes row; text(row, key.origin); append(row, key.local, 4); text(row, source);
    row.insert(row.end(), entry.header.begin(), entry.header.begin() + 4); append(row, entry.offset, 8);
    row.insert(row.end(), entry.header.begin() + 4, entry.header.end());
    option(row, entry.topic); option(row, entry.world); option(row, entry.cell); option(row, entry.child); hash.update(row);
}
static uint32_t float_bits(float value) { uint32_t result; std::memcpy(&result, &value, 4); return result; }
static std::vector<std::string> masters(const Bytes& payload) {
    std::vector<std::string> names; std::set<std::string> unique; bool header = false, pending = false;
    std::optional<size_t> extended; size_t count = 0;
    for (size_t cursor = 0; cursor < payload.size();) {
        if (++count > 1048576 || payload.size() - cursor < 6) throw std::runtime_error("TES4 field header/budget");
        const std::string kind(payload.begin() + cursor, payload.begin() + cursor + 4);
        const size_t short_size = static_cast<size_t>(integer(payload, cursor + 4, 2)); cursor += 6;
        if (kind == "XXXX") { if (extended || short_size != 4) throw std::runtime_error("TES4 XXXX"); extended = static_cast<size_t>(integer(payload, cursor, 4)); cursor += 4; continue; }
        const size_t size = extended.value_or(short_size); extended.reset();
        if (size > payload.size() - cursor) throw std::runtime_error("TES4 field extent");
        if (pending && kind != "DATA") throw std::runtime_error("MAST lacks DATA");
        if (kind == "HEDR") {
            const auto bits = integer(payload, cursor, 4);
            if (header || size != 12 || (bits != float_bits(1.32f) && bits != float_bits(1.33f) && bits != float_bits(1.34f))) throw std::runtime_error("HEDR layout/version"); header = true;
        } else if (kind == "MAST") {
            if (!size || payload[cursor + size - 1] || names.size() >= 254) throw std::runtime_error("MAST extent/termination");
            const std::string name(payload.begin() + cursor, payload.begin() + cursor + size - 1);
            if (!unique.insert(lower(name)).second) throw std::runtime_error("duplicate master"); names.push_back(name); pending = true;
        } else if (kind == "DATA" && pending) { if (size != 8) throw std::runtime_error("MAST DATA length"); pending = false; }
        cursor += size;
    }
    if (extended || pending || !header) throw std::runtime_error("incomplete TES4"); return names;
}
static std::vector<std::string> read_order(const std::filesystem::path& path) {
    std::ifstream input(path, std::ios::binary | std::ios::ate); const auto size = input.tellg();
    if (!input || size < 10 || size > 65536) throw std::runtime_error("order bundle budget");
    Bytes bytes(static_cast<size_t>(size)); input.seekg(0); input.read(reinterpret_cast<char*>(bytes.data()), size);
    if (!input || std::string(bytes.begin(), bytes.begin() + 8) != "FRORDER1") throw std::runtime_error("order magic");
    const auto count = integer(bytes, 8, 2); if (!count || count > 254) throw std::runtime_error("order plugin count");
    size_t cursor = 10; std::vector<std::string> names; std::set<std::string> unique;
    for (size_t i = 0; i < count; ++i) {
        const auto length = static_cast<size_t>(integer(bytes, cursor, 2)); cursor += 2;
        if (length > bytes.size() - cursor) throw std::runtime_error("order name extent");
        const std::string name(bytes.begin() + cursor, bytes.begin() + cursor + length); cursor += length;
        if (!unique.insert(lower(name)).second) throw std::runtime_error("duplicate order plugin"); names.push_back(name);
    }
    if (cursor != bytes.size()) throw std::runtime_error("surplus order bytes"); return names;
}

struct HeaderIndex {
    std::vector<Plugin> plugins; std::map<Key, Entry> winners; uint64_t definitions;
    std::string all_sha256, winners_sha256;
    std::string sources_json() const {
        std::vector<std::string> sources;
        for (const auto& plugin : plugins) sources.push_back(object({{"source_name", quote(plugin.name)}, {"source_bytes", std::to_string(plugin.source->bytes)}, {"source_sha256", quote(plugin.source->sha256)}}));
        return array(sources);
    }
    std::string metadata_json() const {
        return object({{"definitions", std::to_string(definitions)}, {"winning_definitions", std::to_string(winners.size())}, {"all_definitions_sha256", quote(all_sha256)}, {"winning_definitions_sha256", quote(winners_sha256)}});
    }
};
static HeaderIndex scan(const std::filesystem::path& data, const std::filesystem::path& order) {
        const auto names = read_order(order); std::vector<Plugin> plugins; std::set<std::string> loaded;
        std::map<Key, Entry> winners; Hash all; uint64_t definitions = 0, group_count = 0;
        for (const auto& name : names) {
            Plugin plugin{name, {}, std::make_unique<Source>(data / name)};
            const size_t index = plugins.size(); std::vector<Group> groups; std::set<uint32_t> raw_ids; std::set<Key> keys;
            for (uint64_t cursor = 0; cursor < plugin.source->bytes;) {
                while (!groups.empty() && cursor == groups.back().end) groups.pop_back();
                const uint64_t boundary = groups.empty() ? plugin.source->bytes : groups.back().end;
                if (cursor > boundary || boundary - cursor < 24) throw std::runtime_error("group/header boundary");
                const auto header = plugin.source->read(cursor, 24); const std::string kind(header.begin(), header.begin() + 4);
                const auto size = static_cast<uint32_t>(integer(header, 4, 4));
                if (cursor == 0 && kind != "TES4") throw std::runtime_error("missing TES4");
                if (kind == "GRUP") {
                    if (size < 24 || size > boundary - cursor || groups.size() >= 64 || ++group_count > 1000000) throw std::runtime_error("GRUP extent/budget");
                    groups.push_back({cursor + size, static_cast<uint32_t>(integer(header, 8, 4)), static_cast<int32_t>(integer(header, 12, 4))}); cursor += 24; continue;
                }
                if (size > 64 * 1024 * 1024 || size > boundary - cursor - 24) throw std::runtime_error("record extent/budget");
                const auto flags = static_cast<uint32_t>(integer(header, 8, 4)), raw = static_cast<uint32_t>(integer(header, 12, 4));
                if (!cursor) {
                    if (raw || (flags & 0x40000)) throw std::runtime_error("unsupported TES4 header");
                    plugin.masters = masters(plugin.source->read(24, size));
                    for (const auto& master : plugin.masters) if (!loaded.count(lower(master))) throw std::runtime_error("missing or later master");
                } else {
                    if (kind == "TES4" || !raw || !raw_ids.insert(raw).second || ++definitions > 1000000) throw std::runtime_error("record identity/budget");
                    const auto key = *resolve(name, plugin.masters, raw); if (!keys.insert(key).second) throw std::runtime_error("duplicate canonical definition");
                    Entry entry{index, cursor, header};
                    for (const auto& group : groups) {
                        if (group.kind == 1) entry.world = group.label;
                        else if (group.kind == 6) entry.cell = group.label;
                        else if (group.kind == 7) entry.topic = group.label;
                        else if (group.kind >= 8 && group.kind <= 10) { if (entry.cell != group.label) throw std::runtime_error("cell child label mismatch"); entry.child = static_cast<uint32_t>(group.kind); }
                    }
                    metadata(all, key, name, entry); winners.insert_or_assign(key, entry); ++plugin.definitions;
                }
                cursor += 24 + size;
            }
            loaded.insert(lower(name)); plugins.push_back(std::move(plugin));
        }
    Hash winning;
    for (const auto& item : winners) metadata(winning, item.first, plugins[item.second.source].name, item.second);
    return {std::move(plugins), std::move(winners), definitions, all.finish(), winning.finish()};
}
}
