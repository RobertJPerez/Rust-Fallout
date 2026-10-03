// Original ordered association projection directly from offline plugin bytes.
#pragma once
#include "../oracle-common/record_source.hpp"
namespace actor_associations {
using namespace fallout_records;
struct Counts {
    uint64_t records=0,bindings=0,findings=0;
    std::map<std::string,uint64_t> statuses,roles;
    static std::string numbers(const std::map<std::string,uint64_t>& rows) { Object values; for(const auto& row:rows) values[row.first]=std::to_string(row.second); return object(values); }
    std::string json() const { return object({{"records",std::to_string(records)},{"bindings",std::to_string(bindings)},
        {"source_findings",std::to_string(findings)},{"binding_statuses",numbers(statuses)},{"roles",numbers(roles)}}); }
};
struct Target { std::string json,status; std::optional<std::string> kind; };
static Target binding(const HeaderIndex& index,size_t source,uint32_t raw,Counts& counts) {
    const auto& plugin=index.plugins[source]; const auto key=resolve(plugin.name,plugin.masters,raw);
    const auto found=key ? index.winners.find(*key) : index.winners.end();
    std::string status="null",target="null"; std::optional<std::string> kind;
    if(key) {
        if(found==index.winners.end()) status="missing";
        else {
            const auto& entry=found->second; const auto flags=integer(entry.header,8,4);
            status=flags & 0x20 ? "deleted":"defined"; kind=std::string(entry.header.begin(),entry.header.begin()+4);
            std::vector<std::string> signature; for(size_t at=0;at<4;++at) signature.push_back(std::to_string(entry.header[at]));
            target=object({{"kind",array(signature)},{"source_plugin",quote(index.plugins[entry.source].name)},
                {"record_file_offset",std::to_string(entry.offset)},{"record_flags",std::to_string(flags)}});
        }
    }
    ++counts.statuses[status];
    return {object({{"raw_form",std::to_string(raw)},{"key",key ? key->json():"null"},{"status",quote(status)},{"target",target}}),status,kind};
}
struct Document {std::vector<std::string> associations,findings;};
static Document decode(const HeaderIndex& index,const Entry& entry,const Bytes& body,const std::string& record_kind,Counts& counts) {
    Document result; std::optional<size_t> extended; size_t field_index=0; std::map<std::string,size_t> seen;
    const auto finding=[&](const std::string& offset,const std::string& code) {result.findings.push_back(object({{"field_decoded_offset",offset},{"code",quote(code)}}));++counts.findings;};
    for(size_t cursor=0;cursor<body.size();) {
        if(body.size()-cursor<6) throw std::runtime_error("association field header");
        const size_t start=cursor; const std::string signature(body.begin()+cursor,body.begin()+cursor+4);
        const auto short_size=static_cast<size_t>(integer(body,cursor+4,2));cursor+=6;
        if(signature=="XXXX") {if(short_size!=4||extended)throw std::runtime_error("association XXXX");extended=static_cast<size_t>(integer(body,cursor,4));cursor+=4;continue;}
        const auto size=extended.value_or(short_size);extended.reset();
        if(size>body.size()-cursor)throw std::runtime_error("association field extent");
        const Bytes data(body.begin()+cursor,body.begin()+cursor+size);cursor+=size;
        const size_t physical_index=field_index++;
        std::string role;std::set<std::string> kinds;
        if(signature=="SNAM") {role="faction";kinds={"FACT"};}
        else if(signature=="RNAM"&&record_kind=="NPC_") {role="race";kinds={"RACE"};}
        else if(signature=="CNAM"&&record_kind=="NPC_") {role="class";kinds={"CLAS"};}
        else if(signature=="VTCK") {role="voice";kinds={"VTYP"};}
        else if(signature=="INAM") {role="death_item";kinds={"LVLI"};}
        else if(signature=="SPLO") {role="actor_effect";kinds={"SPEL"};}
        else if(signature=="EITM") {role="unarmed_effect";kinds={"ENCH","SPEL"};}
        else if(signature=="PKID") {role="package";kinds={"PACK"};}
        else continue;
        if(size!=(role=="faction" ? 8:4)||counts.bindings>=1000000)throw std::runtime_error("association shape/budget");
        if(++seen[role]>1&&role!="faction"&&role!="actor_effect"&&role!="package")finding(std::to_string(start),"multiple_singleton_associations");
        const auto target=binding(index,entry.source,static_cast<uint32_t>(integer(data,0,4)),counts);
        const std::string allowed=target.kind ? (kinds.count(*target.kind) ? "true":"false") : "null";
        if(target.status=="missing")finding(std::to_string(start),"association_target_missing");
        else if(target.status=="deleted")finding(std::to_string(start),"association_target_deleted");
        else if(allowed=="false")finding(std::to_string(start),"association_target_wrong_kind");
        std::string rank="null",unused="null";
        if(role=="faction") {
            rank=std::to_string(data[4]<128 ? int(data[4]):int(data[4])-256);
            unused=array({std::to_string(data[5]),std::to_string(data[6]),std::to_string(data[7])});
        }
        result.associations.push_back(object({{"field_index",std::to_string(physical_index)},{"role",quote(role)},
            {"binding",target.json},{"schema_kind_allowed",allowed},{"faction_rank",rank},{"faction_unused",unused}}));
        ++counts.bindings;++counts.roles[role];
    }
    if(extended)throw std::runtime_error("association orphan XXXX");
    if(record_kind=="NPC_")for(const auto& role:std::vector<std::string>{"voice","race","class"}) {
        if(!seen.count(role))finding("null","missing_"+role+"_association");
    }
    return result;
}
}
