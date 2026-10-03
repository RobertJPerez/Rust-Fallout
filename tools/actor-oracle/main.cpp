// Original offline source-field comparison. No replacement/runtime code is called.
#include "../oracle-common/record_source.hpp"
#include "../oracle-common/zlib_source.hpp"
#include "associations.hpp"
using namespace fallout_records;

static std::string bytes_json(const Bytes& bytes, size_t first, size_t end) {
    if (first > end || end > bytes.size()) throw std::runtime_error("actor byte array extent");
    std::vector<std::string> rows;
    for (size_t at=first; at<end; ++at) rows.push_back(std::to_string(bytes[at]));
    return array(rows);
}
static std::string kind_json(const Bytes& bytes, size_t first=0) { return bytes_json(bytes,first,first+4); }
static std::string signed_integer(uint64_t word, unsigned width) {
    const auto sign=uint64_t(1) << (width-1);
    return std::to_string(word & sign ? static_cast<int64_t>(word)-static_cast<int64_t>(sign*2) : static_cast<int64_t>(word));
}
struct Counts {
    std::map<std::string,uint64_t> values{{"records",0},{"deleted_records",0},{"decoded_bytes",0},{"fields",0},{"scalar_fields",0},{"source_findings",0}};
    std::map<std::string,uint64_t> kinds,versions,layouts;
    static std::string numbers(const std::map<std::string,uint64_t>& rows) {
        Object values; for (const auto& row:rows) values[row.first]=std::to_string(row.second); return object(values);
    }
    std::string json() const {
        Object rows; for (const auto& row:values) rows[row.first]=std::to_string(row.second);
        rows["record_kinds"]=numbers(kinds); rows["record_versions"]=numbers(versions); rows["scalar_layouts"]=numbers(layouts); return object(rows);
    }
};
struct Document { std::vector<std::string> fields,findings; };
static Document decode(const Bytes& body,const std::string& kind,uint16_t version,Counts& counts) {
    const bool npc=kind=="NPC_";
    const std::set<uint16_t> npc_versions{14,15}, creature_versions{9,11,13,14,15};
    if (!(npc ? npc_versions : creature_versions).count(version)) throw std::runtime_error("unsupported actor version");
    Document result; std::optional<size_t> extended; size_t seen[3]{};
    const auto finding=[&](const std::string& offset,const std::string& code) {
        result.findings.push_back(object({{"field_decoded_offset",offset},{"code",quote(code)}})); ++counts.values["source_findings"];
    };
    for (size_t cursor=0;cursor<body.size();) {
        if (body.size()-cursor<6 || counts.values["fields"]>=2000000) throw std::runtime_error("actor field header/budget");
        const size_t start=cursor;
        const std::string signature(body.begin()+cursor,body.begin()+cursor+4);
        const auto short_size=static_cast<size_t>(integer(body,cursor+4,2)); cursor+=6;
        if (signature=="XXXX") {
            if (short_size!=4 || extended) throw std::runtime_error("actor extended prefix");
            extended=static_cast<size_t>(integer(body,cursor,4)); cursor+=4; continue;
        }
        const auto size=extended.value_or(short_size); extended.reset();
        if (size>body.size()-cursor) throw std::runtime_error("actor field extent");
        const Bytes data(body.begin()+cursor,body.begin()+cursor+size); cursor+=size;
        const auto exact=[&](size_t required) { if (size!=required) throw std::runtime_error("unsupported actor scalar layout"); };
        std::string value=object({{"kind",quote("opaque")}}); int selected=-1;
        if (signature=="ACBS") {
            selected=0; exact(24);
            value=object({{"kind",quote("configuration")},{"fatigue",std::to_string(integer(data,4,2))},
                {"barter_gold",std::to_string(integer(data,6,2))},{"level_word",std::to_string(integer(data,8,2))},
                {"player_level_multiplier_flag",integer(data,0,4) & 0x80 ? "true":"false"},
                {"calc_min",std::to_string(integer(data,10,2))},{"calc_max",std::to_string(integer(data,12,2))},
                {"speed_multiplier",std::to_string(integer(data,14,2))},{"karma_bits",std::to_string(integer(data,16,4))},
                {"disposition_base",signed_integer(integer(data,20,2),16)}});
        } else if (signature=="DATA") {
            selected=1;
            if (npc) {
                if (size<11) throw std::runtime_error("NPC DATA prefix");
                value=object({{"kind",quote("npc_data")},{"base_health",signed_integer(integer(data,0,4),32)},
                    {"attributes",bytes_json(data,4,11)},{"unused_tail",bytes_json(data,11,size)}});
            } else {
                exact(17);
                value=object({{"kind",quote("creature_data")},{"creature_type",std::to_string(data[0])},
                    {"combat_skill",std::to_string(data[1])},{"magic_skill",std::to_string(data[2])},{"stealth_skill",std::to_string(data[3])},
                    {"health",signed_integer(integer(data,4,2),16)},{"unused",bytes_json(data,6,8)},
                    {"damage",signed_integer(integer(data,8,2),16)},{"attributes",bytes_json(data,10,17)}});
            }
        } else if (signature=="DNAM" && npc) {
            selected=2; exact(28);
            value=object({{"kind",quote("npc_skills")},{"skill_values",bytes_json(data,0,14)},{"skill_offsets",bytes_json(data,14,28)}});
        }
        if (selected>=0) {
            const char* codes[]{"multiple_configuration_fields","multiple_actor_data_fields","multiple_npc_skill_fields"};
            if (++seen[selected]>1) finding(std::to_string(start),codes[selected]);
            ++counts.values["scalar_fields"]; ++counts.layouts[kind+":"+signature+":"+std::to_string(size)];
        }
        result.fields.push_back(object({{"kind",kind_json(body,start)},{"decoded_offset",std::to_string(start)},
            {"bytes",std::to_string(size)},{"sha256",quote(fallout_tables::hash(data))},{"value",value}})); ++counts.values["fields"];
    }
    if (extended) throw std::runtime_error("orphan actor extended prefix");
    if (!seen[0]) finding("null","missing_configuration_field");
    if (!seen[1]) finding("null","missing_actor_data_field");
    return result;
}
int wmain(int argc,wchar_t** argv) {
    try {
        if (argc!=3 && argc!=4) throw std::runtime_error("usage: actor-oracle Data_directory FRORDER1_bundle [--include-associations]");
        const bool include_associations=argc==4;
        if(include_associations&&std::wstring(argv[3])!=L"--include-associations")throw std::runtime_error("unknown actor oracle option");
        auto index=scan(argv[1],argv[2]); Counts counts; std::vector<std::string> definitions;
        actor_associations::Counts association_counts;std::vector<std::string> association_definitions;
        for (const auto& winner:index.winners) {
            const auto& entry=winner.second; const auto& source=index.plugins[entry.source];
            const std::string kind(entry.header.begin(),entry.header.begin()+4);
            if (kind!="NPC_" && kind!="CREA") continue;
            if (++counts.values["records"]>65536) throw std::runtime_error("actor record budget"); ++counts.kinds[kind];
            const auto flags=static_cast<uint32_t>(integer(entry.header,8,4)); const bool deleted=flags & 0x20;
            const auto version=static_cast<uint16_t>(integer(entry.header,20,2));
            Document document; std::string body_sha="null";
            actor_associations::Document associations;
            if (deleted) ++counts.values["deleted_records"];
            else {
                auto body=source.source->read(entry.offset+24,static_cast<size_t>(integer(entry.header,4,4)));
                if (flags & 0x40000) {
                    if (body.size()<4) throw std::runtime_error("compressed actor header");
                    const auto expected=static_cast<size_t>(integer(body,0,4));
                    if (expected>64*1024*1024) throw std::runtime_error("compressed actor budget");
                    auto decoded=fallout_zlib::decode(Bytes(body.begin()+4,body.end()),expected);
                    if (decoded.stored_adler!=decoded.calculated_adler) throw std::runtime_error("actor compressed checksum");
                    body=std::move(decoded.payload);
                }
                counts.values["decoded_bytes"]+=body.size();
                if (body.size()>64*1024*1024 || counts.values["decoded_bytes"]>256ULL*1024*1024) throw std::runtime_error("actor decoded byte budget");
                body_sha=quote(fallout_tables::hash(body)); document=decode(body,kind,version,counts); ++counts.versions[kind+":"+std::to_string(version)];
                if(include_associations)associations=actor_associations::decode(index,entry,body,kind,association_counts);
            }
            definitions.push_back(object({{"key",winner.first.json()},{"kind",kind_json(entry.header)},
                {"source",object({{"plugin",quote(source.name)},{"sha256",quote(source.source->sha256)},
                    {"record_file_offset",std::to_string(entry.offset)},{"record_flags",std::to_string(flags)},{"decoded_record_sha256",body_sha}})},
                {"deleted",deleted ? "true":"false"},{"record_version",deleted ? "null":std::to_string(version)},
                {"fields",array(document.fields)},{"findings",array(document.findings)}}));
            if(include_associations){++association_counts.records;association_definitions.push_back(object({{"key",winner.first.json()},
                {"associations",array(associations.associations)},{"findings",array(associations.findings)}}));}
        }
        Object report{{"schema_version","1"},{"profile",quote("nv-original")},{"sources",index.sources_json()},
            {"metadata",index.metadata_json()},{"winning_content_sha256",quote(index.winners_sha256)},
            {"counts",counts.json()},{"definitions",array(definitions)}};
        if(include_associations)report["actor_associations"]=object({{"counts",association_counts.json()},{"definitions",array(association_definitions)}});
        std::cout<<object(report)<<'\n';
        return 0;
    } catch (const std::exception& error) { std::cerr<<"actor-oracle: "<<error.what()<<'\n'; return 1; }
}
