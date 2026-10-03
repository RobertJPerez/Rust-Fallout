// Original independent byte extraction. No game code, zlib implementation or
// Rust decoder is linked. RFC 1950/1951 define the separate format reader.
#include "../oracle-common/record_source.hpp"
#include "../oracle-common/zlib_source.hpp"
using namespace fallout_records;

static Bytes read_bundle(const std::filesystem::path& path) {
    std::ifstream input(path,std::ios::binary|std::ios::ate); const auto size=input.tellg();
    if(!input||size<8||size>256*1024*1024) throw std::runtime_error("frame bundle budget");
    Bytes bytes(static_cast<size_t>(size)); input.seekg(0); input.read(reinterpret_cast<char*>(bytes.data()),size);
    if(!input) throw std::runtime_error("frame bundle read"); return bytes;
}
static std::string signature(const Bytes& bytes) {
    std::ostringstream out;
    for(size_t i=0;i<4;++i) {
        const auto byte=bytes.at(i);
        if(byte>=33&&byte<=126) out<<char(byte);
        else out<<"\\x"<<std::uppercase<<std::hex<<std::setw(2)<<std::setfill('0')<<unsigned(byte);
    }return out.str();
}
static int frames(const std::filesystem::path& path) {
    const auto bytes=read_bundle(path);
    if(std::string(bytes.begin(),bytes.begin()+8)!="FRZLIB01") throw std::runtime_error("frame bundle magic");
    std::vector<std::string> rows; uint64_t decoded_bytes=0,mismatches=0;
    for(size_t cursor=8;cursor<bytes.size();) {
        if(rows.size()>=65536) throw std::runtime_error("frame count budget");
        const auto expected=static_cast<size_t>(integer(bytes,cursor,4));cursor+=4;
        const auto length=static_cast<size_t>(integer(bytes,cursor,4));cursor+=4;
        if(cursor>bytes.size()||length>bytes.size()-cursor) throw std::runtime_error("frame extent");
        Bytes frame(bytes.begin()+cursor,bytes.begin()+cursor+length);cursor+=length;
        const auto decoded=fallout_zlib::decode(frame,expected);decoded_bytes+=decoded.payload.size();
        if(decoded_bytes>4ULL*1024*1024*1024) throw std::runtime_error("frame decoded budget");
        const bool valid=decoded.stored_adler==decoded.calculated_adler;mismatches+=!valid;
        rows.push_back(object({{"index",std::to_string(rows.size())},{"expected_bytes",std::to_string(expected)},
            {"frame_bytes",std::to_string(length)},{"frame_sha256",quote(fallout_tables::hash(frame))},{"decoded_bytes",std::to_string(decoded.payload.size())},
            {"decoded_sha256",quote(fallout_tables::hash(decoded.payload))},{"stored_adler32",std::to_string(decoded.stored_adler)},
            {"calculated_adler32",std::to_string(decoded.calculated_adler)},{"checksum_valid",valid?"true":"false"},
            {"block_counts",array({std::to_string(decoded.blocks[0]),std::to_string(decoded.blocks[1]),std::to_string(decoded.blocks[2])})},{"matches",std::to_string(decoded.matches)}}));
    }
    if(rows.empty()) throw std::runtime_error("empty frame comparison");
    std::cout<<object({{"schema_version","1"},{"bundle_sha256",quote(fallout_tables::hash(bytes))},{"frames",array(rows)},
        {"checksum_mismatches",std::to_string(mismatches)},{"decoded_bytes",std::to_string(decoded_bytes)},{"tainted_payloads_runtime_eligible","false"}})<<'\n';
    return mismatches?1:0;
}
struct Counts {
    uint64_t records=0,stored=0,zlib=0,decoded=0,mismatches=0;std::map<std::string,uint64_t> kinds;
    std::string json()const {
        Object record_kinds;for(const auto& item:kinds)record_kinds[item.first]=std::to_string(item.second);
        return object({{"records",std::to_string(records)},{"stored_bytes",std::to_string(stored)},{"zlib_bytes",std::to_string(zlib)},
            {"decoded_bytes",std::to_string(decoded)},{"checksum_mismatches",std::to_string(mismatches)},{"record_kinds",object(record_kinds)}});
    }
};
static int corpus(const std::filesystem::path& data,const std::filesystem::path& order,bool diagnostic) {
    auto index=scan(data,order);std::vector<std::string> plugins;uint64_t records=0,decoded_bytes=0,mismatches=0;
    for(const auto& plugin:index.plugins) {
        Counts counts;uint64_t groups=0;std::vector<std::string> rows;
        for(uint64_t cursor=0;cursor<plugin.source->bytes;) {
            const auto header=plugin.source->read(cursor,24);const auto size=static_cast<uint32_t>(integer(header,4,4));
            if(std::string(header.begin(),header.begin()+4)=="GRUP"){++groups;cursor+=24;continue;}
            const auto flags=static_cast<uint32_t>(integer(header,8,4)),form=static_cast<uint32_t>(integer(header,12,4));
            if(flags&0x40000) {
                if(size<4||rows.size()>=262144)throw std::runtime_error("compressed record prefix/count budget");
                const auto stored=plugin.source->read(cursor+24,size);const auto expected=static_cast<size_t>(integer(stored,0,4));
                const Bytes zlib(stored.begin()+4,stored.end());const auto decoded=fallout_zlib::decode(zlib,expected);
                const bool valid=decoded.stored_adler==decoded.calculated_adler;
                if(!valid&&!diagnostic)throw std::runtime_error("checksum mismatch in "+plugin.name+" at "+std::to_string(cursor)+" form "+std::to_string(form));
                ++counts.records;counts.stored+=size;counts.zlib+=size-4;counts.decoded+=decoded.payload.size();counts.mismatches+=!valid;
                if(counts.decoded>4ULL*1024*1024*1024)throw std::runtime_error("plugin decoded byte budget");
                const auto kind=signature(header);++counts.kinds[kind];std::string issue="null";
                if(!valid)issue=object({{"file_offset",std::to_string(cursor)},{"form_id",std::to_string(form)},
                    {"stored_adler32",std::to_string(decoded.stored_adler)},{"calculated_adler32",std::to_string(decoded.calculated_adler)}});
                rows.push_back(object({{"record_kind",quote(kind)},{"form_id",std::to_string(form)},{"record_file_offset",std::to_string(cursor)},
                    {"record_flags",std::to_string(flags)},{"stored_bytes",std::to_string(size)},{"decoded_bytes",std::to_string(decoded.payload.size())},
                    {"stored_sha256",quote(fallout_tables::hash(stored))},{"decoded_sha256",quote(fallout_tables::hash(decoded.payload))},{"integrity_issue",issue}}));
            }
            cursor+=24+size;
        }
        records+=counts.records;decoded_bytes+=counts.decoded;mismatches+=counts.mismatches;
        if(records>1000000||decoded_bytes>8ULL*1024*1024*1024)throw std::runtime_error("corpus extraction budget");
        plugins.push_back(object({{"source_name",quote(plugin.name)},{"source_bytes",std::to_string(plugin.source->bytes)},{"source_sha256",quote(plugin.source->sha256)},
            {"record_payloads_decoded",std::to_string(counts.records+1)},{"record_payloads_deferred",std::to_string(plugin.definitions-counts.records)},
            {"groups",std::to_string(groups)},{"counts",counts.json()},{"rows",array(rows)}}));
    }
    std::cout<<object({{"schema_version","1"},{"profile",quote("nv-original")},{"metadata",index.metadata_json()},{"plugins",array(plugins)},
        {"counts",object({{"records",std::to_string(records)},{"decoded_bytes",std::to_string(decoded_bytes)},{"checksum_mismatches",std::to_string(mismatches)}})},
        {"checksum_inspection_enabled",diagnostic?"true":"false"},{"tainted_payloads_runtime_eligible","false"},{"runtime_checksum_policy",quote("strict")},{"retail_parity_accepted","false"}})<<'\n';
    return mismatches?1:0;
}
int wmain(int argc,wchar_t** argv) {
    try {
        if(argc==3&&std::wstring(argv[1])==L"--frames")return frames(argv[2]);
        if((argc==4||argc==5)&&std::wstring(argv[1])==L"--corpus"){
            if(argc==5&&std::wstring(argv[4])!=L"--inspect-checksum-mismatches")throw std::runtime_error("unsupported corpus flag");
            return corpus(argv[2],argv[3],argc==5);
        }
        throw std::runtime_error("usage: zlib-oracle --frames bundle OR --corpus Data_directory order_bundle [--inspect-checksum-mismatches]");
    }catch(const std::exception& error){std::cerr<<"zlib-oracle: "<<error.what()<<'\n';return 1;}
}
