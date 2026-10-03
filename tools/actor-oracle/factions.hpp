// Original offline FACT reader; no runtime or production decoder is called.
#pragma once
#include "body.hpp"
namespace actor_factions {
using namespace fallout_records;
using actor_classes::bytes;
using actor_classes::signed_word;
struct Counts {
    std::map<std::string,uint64_t> values{{"records",0},{"deleted_records",0},{"decoded_bytes",0},{"fields",0},{"selected_fields",0},{"bindings",0},{"source_findings",0}};
    std::map<std::string,uint64_t> versions,layouts;
    actor_associations::Counts binding_counts;
    std::string json()const {
        Object rows;for(const auto& row:values)rows[row.first]=std::to_string(row.second);
        rows["record_versions"]=actor_classes::Counts::numbers(versions);
        rows["selected_layouts"]=actor_classes::Counts::numbers(layouts);
        rows["binding_statuses"]=actor_classes::Counts::numbers(binding_counts.statuses);return object(rows);
    }
};
struct Document {std::vector<std::string> fields,findings;};
static Document decode(const HeaderIndex& index,const Entry& entry,const Bytes& body,uint16_t version,Counts& counts) {
    const std::set<uint16_t> versions{1,2,3,4,8,9,10,11,13,14,15};
    if(!versions.count(version))throw std::runtime_error("unsupported FACT record version");
    Document result;std::optional<size_t> extended;size_t seen[3]{};
    const auto finding=[&](const std::string& offset,const std::string& code){result.findings.push_back(object({{"field_decoded_offset",offset},{"code",quote(code)}}));++counts.values["source_findings"];};
    for(size_t cursor=0;cursor<body.size();) {
        if(body.size()-cursor<6||counts.values["fields"]>=2000000)throw std::runtime_error("faction field header/budget");
        const size_t start=cursor;const std::string kind(body.begin()+cursor,body.begin()+cursor+4);
        const auto short_size=static_cast<size_t>(integer(body,cursor+4,2));cursor+=6;
        if(kind=="XXXX") {
            if(short_size!=4||extended)throw std::runtime_error("faction extended prefix");
            extended=static_cast<size_t>(integer(body,cursor,4));cursor+=4;continue;
        }
        const auto size=extended.value_or(short_size);extended.reset();
        if(size>body.size()-cursor)throw std::runtime_error("faction field extent");
        const Bytes data(body.begin()+cursor,body.begin()+cursor+size);cursor+=size;
        int selected=-1;
        if(kind=="DATA") {selected=0;if(size!=1&&size!=4)throw std::runtime_error("unsupported FACT DATA extent");}
        else if(kind=="CNAM") {selected=1;if(size!=4)throw std::runtime_error("unsupported FACT CNAM extent");}
        else if(kind=="WMI1") {selected=2;if(size!=4)throw std::runtime_error("unsupported FACT WMI1 extent");}
        else if(kind=="XNAM") {selected=3;if(size!=12)throw std::runtime_error("unsupported FACT XNAM extent");}
        else if(kind=="RNAM") {selected=4;if(size!=4)throw std::runtime_error("unsupported FACT RNAM extent");}
        if(selected>=0&&selected<3&&++seen[selected]>1) {
            const char* codes[]{"multiple_faction_data_fields","multiple_faction_unused_float_fields","multiple_faction_reputation_fields"};finding(std::to_string(start),codes[selected]);
        }
        std::string value=object({{"kind",quote("opaque")}});
        if(kind=="DATA")value=object({{"kind",quote("flags")},{"flags_1",std::to_string(data[0])},
            {"flags_2",size==4?std::to_string(data[1]):"null"},{"unused",size==4?bytes(data,2,4):"null"}});
        else if(kind=="CNAM")value=object({{"kind",quote("unused_float")},{"bits",std::to_string(integer(data,0,4))}});
        else if(kind=="RNAM")value=object({{"kind",quote("rank_number")},{"rank",signed_word(integer(data,0,4),32)}});
        else if(kind=="XNAM"||kind=="WMI1") {
            if(counts.values["bindings"]>=1000000)throw std::runtime_error("faction binding budget");
            const bool relation=kind=="XNAM";
            const auto target=actor_associations::binding(index,entry.source,static_cast<uint32_t>(integer(data,0,4)),counts.binding_counts);
            const std::string allowed=target.kind?(relation?(*target.kind=="FACT"||*target.kind=="RACE"?"true":"false"):(*target.kind=="REPU"?"true":"false")):"null";
            if(target.status=="missing")finding(std::to_string(start),"faction_target_missing");
            else if(target.status=="deleted")finding(std::to_string(start),"faction_target_deleted");
            else if(allowed=="false")finding(std::to_string(start),"faction_target_wrong_kind");
            if(relation)value=object({{"kind",quote("relation")},{"faction",target.json},{"modifier",signed_word(integer(data,4,4),32)},
                {"group_combat_reaction",std::to_string(integer(data,8,4))},{"schema_kind_allowed",allowed}});
            else value=object({{"kind",quote("reputation")},{"reputation",target.json},{"schema_kind_allowed",allowed}});
            ++counts.values["bindings"];
        }
        if(selected>=0){++counts.values["selected_fields"];++counts.layouts[kind+":"+std::to_string(size)];}
        result.fields.push_back(object({{"kind",bytes(body,start,start+4)},{"decoded_offset",std::to_string(start)},
            {"bytes",std::to_string(size)},{"sha256",quote(fallout_tables::hash(data))},{"value",value}}));++counts.values["fields"];
    }
    if(extended)throw std::runtime_error("orphan faction extended prefix");
    if(!seen[0])finding("null","missing_faction_data_field");return result;
}
static std::string project(const HeaderIndex& index) {
    Counts counts;std::vector<std::string> definitions;
    for(const auto& winner:index.winners) {
        const auto& entry=winner.second;const auto& source=index.plugins[entry.source];
        if(std::string(entry.header.begin(),entry.header.begin()+4)!="FACT")continue;
        if(++counts.values["records"]>65536)throw std::runtime_error("faction record budget");
        const auto flags=static_cast<uint32_t>(integer(entry.header,8,4));const bool deleted=flags&0x20;
        const auto version=static_cast<uint16_t>(integer(entry.header,20,2));Document document;std::string body_sha="null";
        if(deleted)++counts.values["deleted_records"];
        else {
            auto body=actor_body::read(index,entry,counts.values["decoded_bytes"]);
            body_sha=quote(fallout_tables::hash(body));document=decode(index,entry,body,version,counts);++counts.versions[std::to_string(version)];
        }
        const auto header=object({{"kind",bytes(entry.header,0,4)},{"offset",std::to_string(entry.offset)},
            {"stored_size",std::to_string(integer(entry.header,4,4))},{"flags",std::to_string(flags)},
            {"form_id",std::to_string(integer(entry.header,12,4))},{"revision",bytes(entry.header,16,20)},
            {"version",std::to_string(version)},{"trailing_bytes",bytes(entry.header,22,24)}});
        definitions.push_back(object({{"key",winner.first.json()},{"source",object({{"plugin",quote(source.name)},
            {"sha256",quote(source.source->sha256)},{"record_file_offset",std::to_string(entry.offset)},
            {"record_flags",std::to_string(flags)},{"decoded_record_sha256",body_sha}})},
            {"header",header},{"deleted",deleted?"true":"false"},{"fields",array(document.fields)},{"findings",array(document.findings)}}));
    }
    return object({{"counts",counts.json()},{"definitions",array(definitions)}});
}
}
