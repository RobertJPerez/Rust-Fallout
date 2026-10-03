// Original bounded operand reader for offline comparisons.
#pragma once
#include "../oracle-common/expression_source.hpp"

struct Parameter { uint32_t type, optional; };
struct Signature { uint32_t parse; std::vector<Parameter> parameters; };

static std::vector<Signature> signatures(const SourceImage& source) {
    std::vector<Signature> result;
    for (uint32_t index = 0; index < 640; ++index) {
        const uint32_t address = 0x01190910 + index * 40;
        Signature row{source.number(address + 28, 4), {}};
        const uint32_t count = source.number(address + 18, 2);
        if (count > 64) throw std::runtime_error("signature count budget");
        const uint32_t parameters = source.number(address + 20, 4);
        for (uint32_t p = 0; p < count; ++p) {
            row.parameters.push_back({source.number(parameters + p * 12 + 4, 4),
                source.number(parameters + p * 12 + 8, 4)});
        }
        result.push_back(std::move(row));
    }
    return result;
}

static bool message_parser(uint32_t address) {
    return address == 0x005b3c70 || address == 0x005b3ca0
        || address == 0x005b3c40 || address == 0x005b3cd0;
}

struct Value {
    size_t start = 0, end = 0;
    uint8_t tag = 0, type = 0;
    uint16_t index = 0;
    std::optional<uint16_t> context;
    uint64_t bits = 0;
    std::optional<Bytes> payload;
};

static void extension_guard(const Bytes& bytes, size_t at) {
    if (at < bytes.size() && bytes.size() - at >= 2 && number(bytes, at, 2) == 0xffff)
        throw std::runtime_error("unsupported extension argument");
}

static Value local(const Bytes& bytes, size_t& at) {
    Value value; value.tag = 7;
    auto prefix = number(bytes, at++, 1);
    if (prefix == 'r') {
        value.context = word(bytes, at);
        prefix = number(bytes, at++, 1);
    }
    if (prefix != 'f' && prefix != 's') throw std::runtime_error("variable prefix");
    value.type = static_cast<uint8_t>(prefix);
    value.index = word(bytes, at);
    return value;
}

static Value numeric(const Bytes& bytes, size_t& at) {
    extension_guard(bytes, at);
    const size_t start = at;
    const auto prefix = number(bytes, at++, 1);
    Value value;
    if (prefix == 'n') {
        value.tag = 2; value.bits = number(bytes, at, 4); at += 4;
    } else if (prefix == 'z') {
        value.tag = 3;
        // Retain both words; a host floating conversion would lose evidence.
        value.bits = uint64_t(number(bytes, at, 4))
            | (uint64_t(number(bytes, at + 4, 4)) << 32);
        at += 8;
    } else if (prefix == 'G') {
        value.tag = 6; value.index = word(bytes, at);
    } else {
        at = start; value = local(bytes, at);
    }
    return value;
}

static Value operand(const Bytes& bytes, size_t& at, uint32_t type) {
    extension_guard(bytes, at);
    Value value;
    switch (type) {
    case 0: {
        value.tag = 1;
        const auto count = word(bytes, at);
        if (count > 512) throw std::runtime_error("string budget");
        value.payload = take(bytes, at, count);
        break;
    }
    case 1: case 2: case 23: case 44:
        value = numeric(bytes, at); break;
    case 5: case 10: case 18: case 28: case 41: case 51: case 52: case 55:
        value.tag = 4; value.bits = word(bytes, at); break;
    case 8: case 32:
        value.tag = 5; value.bits = number(bytes, at++, 1); break;
    case 45:
        value = local(bytes, at); break;
    case 22: case 46:
        throw std::runtime_error("unsupported parameter classification");
    default: {
        if (type >= 70) throw std::runtime_error("unknown parameter type");
        const auto prefix = number(bytes, at++, 1);
        if (prefix != 'r' && prefix != 'f') throw std::runtime_error("form prefix");
        value.tag = prefix == 'r' ? 8 : 9;
        value.index = word(bytes, at);
    }
    }
    return value;
}

static void append64(Bytes& bytes, uint64_t bits) {
    append(bytes, static_cast<uint32_t>(bits), 4);
    append(bytes, static_cast<uint32_t>(bits >> 32), 4);
}

static Bytes value_tuple(const Value& value, uint32_t type) {
    Bytes bytes;
    append(bytes, static_cast<uint32_t>(value.start), 4);
    append(bytes, static_cast<uint32_t>(value.end), 4);
    append(bytes, type, 4);
    append(bytes, value.tag, 1); append(bytes, value.type, 1);
    append(bytes, value.index, 2);
    append(bytes, value.context.has_value(), 1);
    append(bytes, value.context.value_or(0), 2);
    append64(bytes, value.bits);
    append(bytes, value.payload ? static_cast<uint32_t>(value.payload->size()) : 0, 4);
    if (value.payload) {
        const auto hash = digest(*value.payload);
        for (size_t i = 0; i < hash.size(); i += 2)
            bytes.push_back(static_cast<unsigned char>(std::stoul(hash.substr(i, 2), nullptr, 16)));
    } else bytes.resize(bytes.size() + 32, 0);
    if (bytes.size() != 63) throw std::runtime_error("argument tuple invariant");
    return bytes;
}

