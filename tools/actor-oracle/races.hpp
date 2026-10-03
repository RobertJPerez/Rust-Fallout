// Independent original RACE scalar projection; no production decoder invoked.
#pragma once
#include "body.hpp"
namespace actor_races {
using namespace fallout_records;
using actor_classes::bytes;
using actor_classes::signed_word;
using Counts=actor_classes::Counts;
struct Document {std::vector<std::string> fields,findings;};
static Document decode(const Bytes& body,Counts& counts){
    Document result;std::optional<size_t> extended;size_t seen[3]{};
    const char* duplicate[]{"multiple_race_data_fields","multiple_race_main_clamp_fields","multiple_race_face_clamp_fields"};
    const char* missing[]{"missing_race_data_field","missing_race_main_clamp_field","missing_race_face_clamp_field"};
    const auto finding=[&](const std::string& offset,const std::string& code){result.findings.push_back(object({{"field_decoded_offset",offset},{"code",quote(code)}}));++counts.values["source_findings"];};
    for(size_t cursor=0;cursor<body.size();){
        if(body.size()-cursor<6||counts.values["fields"]>=2000000)throw std::runtime_error("race field header/budget");
        const size_t start=cursor;const std::string kind(body.begin()+cursor,body.begin()+cursor+4);
        const auto short_size=static_cast<size_t>(integer(body,cursor+4,2));cursor+=6;
        if(kind=="XXXX"){
            if(short_size!=4||extended)throw std::runtime_error("race extended prefix");
            extended=static_cast<size_t>(integer(body,cursor,4));cursor+=4;continue;
        }
        const auto size=extended.value_or(short_size);extended.reset();
        if(size>body.size()-cursor)throw std::runtime_error("race field extent");
        int selected=-1;
        if(kind=="DATA"){if(size!=36)throw std::runtime_error("unsupported RACE DATA extent");selected=0;}
        else if(kind=="PNAM"){if(size!=4)throw std::runtime_error("unsupported RACE PNAM extent");selected=1;}
        else if(kind=="UNAM"){if(size!=4)throw std::runtime_error("unsupported RACE UNAM extent");selected=2;}
        const Bytes data(body.begin()+cursor,body.begin()+cursor+size);cursor+=size;
        std::string value=object({{"kind",quote("opaque")}});
        if(selected==0){
            std::vector<std::string> boosts;
            for(size_t at=0;at<14;at+=2)boosts.push_back(object({{"skill",signed_word(data[at],8)},{"boost",signed_word(data[at+1],8)}}));
            value=object({{"kind",quote("race_data")},{"skill_boosts",array(boosts)},{"unused",bytes(data,14,16)},
                {"height_bits",array({std::to_string(integer(data,16,4)),std::to_string(integer(data,20,4))})},
                {"weight_bits",array({std::to_string(integer(data,24,4)),std::to_string(integer(data,28,4))})},
                {"flags",std::to_string(integer(data,32,4))}});
        }else if(selected>=1)value=object({{"kind",quote(selected==1?"main_clamp":"face_clamp")},{"bits",std::to_string(integer(data,0,4))}});
        if(selected>=0){
            if(++seen[selected]>1)finding(std::to_string(start),duplicate[selected]);
            ++counts.values["scalar_fields"];++counts.layouts[kind+":"+std::to_string(size)];
        }
        result.fields.push_back(object({{"kind",bytes(body,start,start+4)},{"decoded_offset",std::to_string(start)},{"bytes",std::to_string(size)},{"sha256",quote(fallout_tables::hash(data))},{"value",value}}));++counts.values["fields"];
    }
    if(extended)throw std::runtime_error("orphan race extended prefix");
    for(size_t at=0;at<3;++at)if(!seen[at])finding("null",missing[at]);
    return result;
}
static std::string project(const HeaderIndex& index){
    Counts counts;std::vector<std::string> definitions;
    for(const auto& winner:index.winners){
        const auto& entry=winner.second;const auto& source=index.plugins[entry.source];
        if(std::string(entry.header.begin(),entry.header.begin()+4)!="RACE")continue;
        if(++counts.values["records"]>65536)throw std::runtime_error("race record budget");
        const auto flags=static_cast<uint32_t>(integer(entry.header,8,4));const bool deleted=flags&0x20;
        const auto version=static_cast<uint16_t>(integer(entry.header,20,2));Document document;std::string body_sha="null";
        if(deleted)++counts.values["deleted_records"];
        else{
            if(version!=15)throw std::runtime_error("unsupported RACE record version");
            auto body=actor_body::read(index,entry,counts.values["decoded_bytes"]);
            body_sha=quote(fallout_tables::hash(body));document=decode(body,counts);++counts.versions[std::to_string(version)];
        }
        const auto header=object({{"kind",bytes(entry.header,0,4)},{"offset",std::to_string(entry.offset)},{"stored_size",std::to_string(integer(entry.header,4,4))},
            {"flags",std::to_string(flags)},{"form_id",std::to_string(integer(entry.header,12,4))},{"revision",bytes(entry.header,16,20)},
            {"version",std::to_string(version)},{"trailing_bytes",bytes(entry.header,22,24)}});
        definitions.push_back(object({{"key",winner.first.json()},{"source",object({{"plugin",quote(source.name)},{"sha256",quote(source.source->sha256)},
            {"record_file_offset",std::to_string(entry.offset)},{"record_flags",std::to_string(flags)},{"decoded_record_sha256",body_sha}})},
            {"header",header},{"deleted",deleted?"true":"false"},{"fields",array(document.fields)},{"findings",array(document.findings)}}));
    }
    return object({{"counts",counts.json()},{"definitions",array(definitions)}});
}
}
