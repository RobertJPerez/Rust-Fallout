// Independent direct-source PACK projection; no production decoder invoked.
#pragma once
#include "body.hpp"
namespace actor_packages {
using namespace fallout_records;
using actor_classes::bytes;
using actor_classes::signed_word;
using Counts=actor_classes::Counts;
struct Document {std::vector<std::string> fields,findings;};
static Document decode(const Bytes& body,Counts& counts){
    Document result;std::optional<size_t> extended;size_t seen[2]{};
    const char* duplicate[]{"multiple_package_general_fields","multiple_package_schedule_fields"};
    const char* missing[]{"missing_package_general_field","missing_package_schedule_field"};
    const auto finding=[&](const std::string& offset,const std::string& code){result.findings.push_back(object({{"field_decoded_offset",offset},{"code",quote(code)}}));++counts.values["source_findings"];};
    for(size_t cursor=0;cursor<body.size();){
        if(body.size()-cursor<6||counts.values["fields"]>=2000000)throw std::runtime_error("package field header/budget");
        const size_t start=cursor;const std::string kind(body.begin()+cursor,body.begin()+cursor+4);
        const auto short_size=static_cast<size_t>(integer(body,cursor+4,2));cursor+=6;
        if(kind=="XXXX"){
            if(short_size!=4||extended)throw std::runtime_error("package extended prefix");
            extended=static_cast<size_t>(integer(body,cursor,4));cursor+=4;continue;
        }
        const auto size=extended.value_or(short_size);extended.reset();
        if(size>body.size()-cursor)throw std::runtime_error("package field extent");
        int selected=-1;
        if(kind=="PKDT"){if(size!=8&&size!=12)throw std::runtime_error("unsupported PACK PKDT extent");selected=0;}
        else if(kind=="PSDT"){if(size!=8)throw std::runtime_error("unsupported PACK PSDT extent");selected=1;}
        const Bytes data(body.begin()+cursor,body.begin()+cursor+size);cursor+=size;
        std::string value=object({{"kind",quote("opaque")}});
        if(selected==0){
            const auto tail=size==12?object({{"type_specific_flags",std::to_string(integer(data,8,2))},{"unused",bytes(data,10,12)}}):"null";
            value=object({{"kind",quote("general")},{"flags",std::to_string(integer(data,0,4))},
                {"package_type",std::to_string(data[4])},{"unused",std::to_string(data[5])},
                {"behavior_flags",std::to_string(integer(data,6,2))},{"tail",tail}});
        }else if(selected==1){
            value=object({{"kind",quote("schedule")},{"month",signed_word(data[0],8)},
                {"weekday",signed_word(data[1],8)},{"date",signed_word(data[2],8)},
                {"hour",signed_word(data[3],8)},{"duration",std::to_string(integer(data,4,4))}});
        }
        if(selected>=0){
            if(++seen[selected]>1)finding(std::to_string(start),duplicate[selected]);
            ++counts.values["scalar_fields"];++counts.layouts[kind+":"+std::to_string(size)];
        }
        result.fields.push_back(object({{"kind",bytes(body,start,start+4)},{"decoded_offset",std::to_string(start)},{"bytes",std::to_string(size)},{"sha256",quote(fallout_tables::hash(data))},{"value",value}}));++counts.values["fields"];
    }
    if(extended)throw std::runtime_error("orphan package extended prefix");
    for(size_t at=0;at<2;++at)if(!seen[at])finding("null",missing[at]);
    return result;
}
static std::string project(const HeaderIndex& index){
    Counts counts;std::vector<std::string> definitions;
    const std::set<uint16_t> versions{1,2,3,9,10,11,13,14,15};
    for(const auto& winner:index.winners){
        const auto& entry=winner.second;const auto& source=index.plugins[entry.source];
        if(std::string(entry.header.begin(),entry.header.begin()+4)!="PACK")continue;
        if(++counts.values["records"]>65536)throw std::runtime_error("package record budget");
        const auto flags=static_cast<uint32_t>(integer(entry.header,8,4));const bool deleted=flags&0x20;
        const auto version=static_cast<uint16_t>(integer(entry.header,20,2));Document document;std::string body_sha="null";
        if(deleted)++counts.values["deleted_records"];
        else{
            if(!versions.count(version))throw std::runtime_error("unsupported PACK record version");
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
