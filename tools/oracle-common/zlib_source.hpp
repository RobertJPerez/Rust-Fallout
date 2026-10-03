// Original bounded offline decoder derived from RFC 1950/1951's format rules.
// Runtime Rust continues to use its pinned compression library. This reader is
// separate evidence, with no imported zlib implementation or copied sample code.
#pragma once
#include <algorithm>
#include <array>
#include <cstdint>
#include <cstddef>
#include <optional>
#include <stdexcept>
#include <vector>

namespace fallout_zlib {
using std::size_t;
using Bytes = std::vector<unsigned char>;
class Bits {
    const Bytes& bytes; size_t first, end, position = 0;
public:
    Bits(const Bytes& bytes, size_t first, size_t end) : bytes(bytes), first(first), end(end) {
        if (first > end || end > bytes.size()) throw std::runtime_error("deflate input extent");
    }
    size_t remaining() const { return (end - first) * 8 - position; }
    uint32_t peek(unsigned count) const {
        if (count > 16) throw std::runtime_error("bit word width");
        count = static_cast<unsigned>(std::min<size_t>(count, remaining()));
        uint32_t word = 0;
        for (unsigned bit = 0; bit < count; ++bit) {
            const auto at = position + bit;
            word |= uint32_t((bytes[first + at / 8] >> (at % 8)) & 1) << bit;
        }
        return word;
    }
    uint32_t read(unsigned count) {
        if (count > remaining()) throw std::runtime_error("truncated deflate bits");
        const auto word = peek(count); position += count; return word;
    }
    void align() { position = (position + 7) & ~size_t(7); if (position > (end - first) * 8) throw std::runtime_error("deflate alignment"); }
    void copy_stored(Bytes& output, size_t length) {
        if (position % 8 || length > remaining() / 8) throw std::runtime_error("stored block extent");
        const size_t start = first + position / 8;
        output.insert(output.end(), bytes.begin() + start, bytes.begin() + start + length); position += length * 8;
    }
    size_t consumed_bytes() const { return (position + 7) / 8; }
};
class Huffman {
    struct Entry { uint16_t symbol = 0; uint8_t width = 0; };
    std::vector<Entry> table; unsigned width = 0;
public:
    Huffman(const std::vector<uint8_t>& lengths, unsigned maximum, bool empty_allowed, bool single_allowed) {
        if (!maximum || maximum > 15) throw std::runtime_error("Huffman maximum width");
        std::array<uint32_t,16> counts{}, next{}; size_t symbols = 0;
        for (const auto length : lengths) {
            if (length > maximum || length > 15) throw std::runtime_error("Huffman code width");
            if (length) { ++counts[length]; ++symbols; width = std::max(width, unsigned(length)); }
        }
        if (!symbols) { if (empty_allowed) return; throw std::runtime_error("empty Huffman alphabet"); }
        int32_t free = 1;
        for (unsigned bits = 1; bits <= maximum; ++bits) {
            free = free * 2 - static_cast<int32_t>(counts[bits]);
            if (free < 0) throw std::runtime_error("oversubscribed Huffman tree");
        }
        if (free && !(single_allowed && symbols == 1 && width == 1)) throw std::runtime_error("incomplete Huffman tree");
        uint32_t prefix = 0;
        for (unsigned bits = 1; bits <= maximum; ++bits) { prefix = (prefix + counts[bits - 1]) * 2; next[bits] = prefix; }
        table.resize(size_t(1) << width);
        for (size_t symbol = 0; symbol < lengths.size(); ++symbol) {
            const unsigned length = lengths[symbol]; if (!length) continue;
            const uint32_t code = next[length]++; uint32_t reversed = 0;
            for (unsigned bit = 0; bit < length; ++bit) reversed |= ((code >> bit) & 1) << (length - bit - 1);
            for (size_t at = reversed; at < table.size(); at += size_t(1) << length) table[at] = {static_cast<uint16_t>(symbol), static_cast<uint8_t>(length)};
        }
    }
    uint16_t read(Bits& bits) const {
        if (!width) throw std::runtime_error("distance requested from empty alphabet");
        const auto entry = table[bits.peek(width)];
        if (!entry.width || entry.width > bits.remaining()) throw std::runtime_error("invalid or truncated Huffman symbol");
        (void)bits.read(entry.width); return entry.symbol;
    }
};
struct Trees { Huffman literals, distances; };
static const Trees& fixed() {
    static const Trees trees = [] {
        std::vector<uint8_t> literals(288);
        for (size_t symbol = 0; symbol < literals.size(); ++symbol) literals[symbol] = symbol < 144 ? 8 : symbol < 256 ? 9 : symbol < 280 ? 7 : 8;
        return Trees{Huffman(literals,15,false,false),Huffman(std::vector<uint8_t>(32,5),15,false,false)};
    }();
    return trees;
}
static Trees dynamic(Bits& bits) {
    const size_t literal_count = bits.read(5) + 257, distance_count = bits.read(5) + 1, code_count = bits.read(4) + 4;
    if (literal_count > 286) throw std::runtime_error("dynamic literal alphabet size");
    constexpr std::array<unsigned,19> order{16,17,18,0,8,7,9,6,10,5,11,4,12,3,13,2,14,1,15};
    std::vector<uint8_t> code_lengths(19);
    for (size_t i = 0; i < code_count; ++i) code_lengths[order[i]] = static_cast<uint8_t>(bits.read(3));
    const Huffman codes(code_lengths,7,false,false); std::vector<uint8_t> lengths;
    const size_t total = literal_count + distance_count; lengths.reserve(total);
    while (lengths.size() < total) {
        const auto symbol = codes.read(bits); uint8_t length = 0; size_t repeat = 1;
        if (symbol < 16) length = static_cast<uint8_t>(symbol);
        else if (symbol == 16) {
            if (lengths.empty()) throw std::runtime_error("repeat has no previous length");
            length = lengths.back(); repeat = bits.read(2) + 3;
        } else if (symbol == 17) repeat = bits.read(3) + 3;
        else if (symbol == 18) repeat = bits.read(7) + 11;
        else throw std::runtime_error("code length symbol");
        if (repeat > total - lengths.size()) throw std::runtime_error("code length repeat overflow");
        lengths.insert(lengths.end(),repeat,length);
    }
    if (!lengths[256]) throw std::runtime_error("literal alphabet has no end marker");
    std::vector<uint8_t> literals(lengths.begin(),lengths.begin()+literal_count), distances(lengths.begin()+literal_count,lengths.end());
    return {Huffman(literals,15,false,true),Huffman(distances,15,true,true)};
}
static uint32_t adler32(const Bytes& bytes) {
    uint32_t sum = 1, weighted = 0;
    // A 4096-byte chunk keeps the worst-case weighted sum below UINT32_MAX.
    for (size_t start = 0; start < bytes.size();) {
        const size_t end = std::min(bytes.size(),start+4096);
        for (;start<end;++start) { sum += bytes[start]; weighted += sum; }
        sum %= 65521; weighted %= 65521;
    }
    return (weighted << 16) | sum;
}
struct Decoded { Bytes payload; uint32_t stored_adler, calculated_adler; std::array<uint64_t,3> blocks{}; uint64_t matches = 0; };
static Decoded decode(const Bytes& bytes, size_t expected, size_t maximum = 64*1024*1024) {
    if (bytes.size() < 6 || bytes.size() > maximum || expected > maximum) throw std::runtime_error("zlib byte budget");
    if ((bytes[0]&15)!=8 || (bytes[0]>>4)>7 || (unsigned(bytes[0])*256+bytes[1])%31) throw std::runtime_error("zlib header");
    if (bytes[1]&32) throw std::runtime_error("preset dictionary is unsupported");
    const size_t window = size_t(1) << ((bytes[0]>>4)+8);
    Bits bits(bytes,2,bytes.size()-4); Decoded result; result.payload.reserve(expected);
    constexpr std::array<uint16_t,29> lengths{3,4,5,6,7,8,9,10,11,13,15,17,19,23,27,31,35,43,51,59,67,83,99,115,131,163,195,227,258};
    constexpr std::array<uint8_t,29> length_extra{0,0,0,0,0,0,0,0,1,1,1,1,2,2,2,2,3,3,3,3,4,4,4,4,5,5,5,5,0};
    constexpr std::array<uint16_t,30> distances{1,2,3,4,5,7,9,13,17,25,33,49,65,97,129,193,257,385,513,769,1025,1537,2049,3073,4097,6145,8193,12289,16385,24577};
    constexpr std::array<uint8_t,30> distance_extra{0,0,0,0,1,1,2,2,3,3,4,4,5,5,6,6,7,7,8,8,9,9,10,10,11,11,12,12,13,13};
    bool final = false; size_t block_count = 0;
    while (!final) {
        if (++block_count > 1000000) throw std::runtime_error("deflate block budget");
        final = bits.read(1)!=0; const auto type = bits.read(2);
        if (type==3) throw std::runtime_error("reserved deflate block type"); ++result.blocks[type];
        if (!type) {
            bits.align(); const auto length = bits.read(16), complement = bits.read(16);
            if ((length^complement)!=65535 || length > expected-result.payload.size()) throw std::runtime_error("stored block length");
            bits.copy_stored(result.payload,length); continue;
        }
        std::optional<Trees> authored;
        if (type==2) authored.emplace(dynamic(bits));
        const auto& trees = authored ? *authored : fixed();
        for (;;) {
            const auto symbol = trees.literals.read(bits);
            if (symbol==256) break;
            if (symbol<256) {
                if (result.payload.size()>=expected) throw std::runtime_error("literal exceeds decoded extent"); result.payload.push_back(static_cast<unsigned char>(symbol));
            } else {
                if (symbol>285) throw std::runtime_error("reserved length symbol");
                const size_t length_index = symbol-257; const size_t length = lengths[length_index]+bits.read(length_extra[length_index]);
                const auto distance_symbol = trees.distances.read(bits); if (distance_symbol>=30) throw std::runtime_error("reserved distance symbol");
                const size_t distance = distances[distance_symbol]+bits.read(distance_extra[distance_symbol]);
                if (distance>result.payload.size() || distance>window || length>expected-result.payload.size()) throw std::runtime_error("match distance or decoded extent");
                for (size_t i=0;i<length;++i) result.payload.push_back(result.payload[result.payload.size()-distance]); ++result.matches;
            }
        }
    }
    if (result.payload.size()!=expected || bits.consumed_bytes()!=bytes.size()-6) throw std::runtime_error("decoded size or surplus deflate bytes");
    result.stored_adler=0; for (size_t at=bytes.size()-4;at<bytes.size();++at) result.stored_adler=(result.stored_adler<<8)|bytes[at];
    result.calculated_adler=adler32(result.payload); return result;
}
}
