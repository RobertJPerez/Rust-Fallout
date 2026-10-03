// Strict source-body bounds shared only by the independent offline projections.
#pragma once
#include "../oracle-common/record_source.hpp"
#include "../oracle-common/zlib_source.hpp"
namespace actor_body {
using namespace fallout_records;
constexpr uint64_t record_limit=64ULL*1024*1024;
constexpr uint64_t catalogue_limit=256ULL*1024*1024;

// Callbacks permit allocation-order checks without creating enormous files or
// allocating a hostile declared payload. Normal reads use the locked Source.
template<class Reader,class Decoder>
static Bytes bounded(size_t stored,uint32_t flags,uint64_t& used,Reader read,Decoder decode) {
    if(used>catalogue_limit)throw std::runtime_error("actor source cumulative body budget");
    const auto maximum=static_cast<size_t>(std::min(record_limit,catalogue_limit-used));
    if(stored>maximum)throw std::runtime_error("actor source stored body budget before read");
    Bytes body=read(stored);
    if(body.size()!=stored)throw std::runtime_error("actor source incomplete stored read");
    if(flags&0x40000) {
        if(body.size()<4)throw std::runtime_error("actor source compressed body header");
        const auto expected=static_cast<size_t>(integer(body,0,4));
        if(expected>maximum)throw std::runtime_error("actor source declared body budget before inflate");
        auto decoded=decode(Bytes(body.begin()+4,body.end()),expected,maximum);
        if(decoded.stored_adler!=decoded.calculated_adler||decoded.payload.size()!=expected)throw std::runtime_error("actor source compressed integrity");
        body=std::move(decoded.payload);
    }
    if(body.size()>maximum)throw std::runtime_error("actor source decoded body budget");
    used+=body.size();
    return body;
}
static Bytes read(const HeaderIndex& index,const Entry& entry,uint64_t& used) {
    const auto flags=static_cast<uint32_t>(integer(entry.header,8,4));
    const auto stored=static_cast<size_t>(integer(entry.header,4,4));
    return bounded(stored,flags,used,
        [&](size_t size){return index.plugins[entry.source].source->read(entry.offset+24,size);},
        [](const Bytes& compressed,size_t expected,size_t maximum){return fallout_zlib::decode(compressed,expected,maximum);});
}
}
