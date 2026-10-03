// Original offline placed-actor projection from winning plugin bytes.
#pragma once
namespace actor_placements {
using namespace fallout_records;
using actor_classes::bytes;
struct Counts {
    std::map<std::string,uint64_t> values{{"records",0},{"deleted_records",0},{"decoded_bytes",0},{"fields",0},{"selected_extra_fields",0},{"bindings",0},{"source_findings",0}};
    std::map<std::string,uint64_t> kinds,versions,layouts;
    actor_associations::Counts binding_counts;
    std::string json()const{Object rows;for(const auto& row:values)rows[row.first]=std::to_string(row.second);rows["record_kinds"]=actor_classes::Counts::numbers(kinds);rows["record_versions"]=actor_classes::Counts::numbers(versions);rows["extra_layouts"]=actor_classes::Counts::numbers(layouts);rows["binding_statuses"]=actor_classes::Counts::numbers(binding_counts.statuses);return object(rows);}
};
struct Document {
    std::vector<std::string> fields;
    std::vector<std::pair<size_t,std::string>> findings;
    std::string core="null",base="null",allowed="null";
    std::string findings_json(){std::stable_sort(findings.begin(),findings.end(),[](const auto&a,const auto&b){return a.first<b.first;});std::vector<std::string> rows;for(const auto& row:findings)rows.push_back(row.second);return array(rows);}
};
static bool finite_word(uint32_t bits){return (bits&0x7f800000)!=0x7f800000;}
static std::string words(const Bytes& data,size_t start,size_t count){std::vector<std::string> rows;for(size_t at=0;at<count;++at)rows.push_back(std::to_string(integer(data,start+at*4,4)));return array(rows);}
static Document decode(const HeaderIndex& index,const Entry& entry,const Bytes& body,const std::string& kind,uint16_t version,Counts& counts) {
    if(kind=="ACHR"?version!=15:(version!=9&&version!=11&&version!=15))throw std::runtime_error("unsupported placed actor record version");
    Document result;std::optional<size_t> extended;size_t core_seen[5]{},extra_seen[3]{};
    std::string base_field="null",positions="null",rotations="null",scale="null";size_t transform_offset=0;
    const auto finding=[&](size_t offset,const std::string& code){result.findings.push_back({offset,object({{"field_decoded_offset",std::to_string(offset)},{"code",quote(code)}})});++counts.values["source_findings"];};
    const auto binding=[&](uint32_t raw,const std::string& expected,size_t offset){
        if(counts.values["bindings"]>=1000000)throw std::runtime_error("placed actor binding budget");
        const auto target=actor_associations::binding(index,entry.source,raw,counts.binding_counts);++counts.values["bindings"];
        const std::string allowed=target.kind?(*target.kind==expected?"true":"false"):"null";
        if(target.status=="missing")finding(offset,"placement_target_missing");else if(target.status=="deleted")finding(offset,"placement_target_deleted");else if(allowed=="false")finding(offset,"placement_target_wrong_kind");
        return std::make_pair(target.json,allowed);
    };
    for(size_t cursor=0;cursor<body.size();) {
        if(body.size()-cursor<6||counts.values["fields"]>=2000000)throw std::runtime_error("placed actor field header/budget");
        const size_t start=cursor;const std::string signature(body.begin()+cursor,body.begin()+cursor+4);
        const auto short_size=static_cast<size_t>(integer(body,cursor+4,2));cursor+=6;
        if(signature=="XXXX"){if(short_size!=4||extended)throw std::runtime_error("placed actor extended prefix");extended=static_cast<size_t>(integer(body,cursor,4));cursor+=4;continue;}
        const auto size=extended.value_or(short_size);extended.reset();if(size>body.size()-cursor)throw std::runtime_error("placed actor field extent");
        const Bytes data(body.begin()+cursor,body.begin()+cursor+size);cursor+=size;
        int core_index=-1;size_t core_size=0;
        if(signature=="NAME"){core_index=0;core_size=4;}else if(signature=="DATA"){core_index=1;core_size=24;}else if(signature=="XSCL"){core_index=2;core_size=4;}else if(signature=="XESP"){core_index=3;core_size=8;}else if(signature=="XTEL"){core_index=4;core_size=32;}
        if(core_index>=0&&(size!=core_size||++core_seen[core_index]>1))throw std::runtime_error("placed actor core shape/duplicate");
        if(signature=="NAME"){
            const auto raw=static_cast<uint32_t>(integer(data,0,4));base_field=object({{"decoded_offset",std::to_string(start)},{"value",std::to_string(raw)}});
            const auto target=binding(raw,kind=="ACHR"?"NPC_":"CREA",start);result.base=target.first;result.allowed=target.second;
        }else if(signature=="DATA"){
            for(size_t at=0;at<24;at+=4)if(!finite_word(static_cast<uint32_t>(integer(data,at,4))))throw std::runtime_error("nonfinite placed actor transform");
            positions=words(data,0,3);rotations=words(data,12,3);transform_offset=start;
        }else if(signature=="XSCL"){
            const auto bits=static_cast<uint32_t>(integer(data,0,4));if(!finite_word(bits)||(bits&0x80000000)||!(bits&0x7fffffff))throw std::runtime_error("invalid placed actor scale");
            scale=object({{"decoded_offset",std::to_string(start)},{"value",std::to_string(bits)}});
        }else if(signature=="XTEL")for(size_t at=4;at<28;at+=4)if(!finite_word(static_cast<uint32_t>(integer(data,at,4))))throw std::runtime_error("nonfinite placed actor teleport");
        int selected=-1;if(signature=="XEZN")selected=0;else if(signature=="XMRC")selected=1;else if(signature=="XLCM")selected=2;
        std::string value=object({{"kind",quote("opaque")}});
        if(selected>=0){
            if(size!=4)throw std::runtime_error("unsupported placed actor extra extent");
            if(++extra_seen[selected]>1){const char* codes[]{"multiple_placed_encounter_zone_fields","multiple_placed_merchant_fields","multiple_placed_level_modifier_fields"};finding(start,codes[selected]);}
            if(selected==2)value=object({{"kind",quote("level_modifier")},{"modifier",actor_classes::signed_word(integer(data,0,4),32)}});
            else{const auto target=binding(static_cast<uint32_t>(integer(data,0,4)),selected==0?"ECZN":"REFR",start);value=object({{"kind",quote(selected==0?"encounter_zone":"merchant_container")},{selected==0?"zone":"container",target.first},{"schema_kind_allowed",target.second}});}
            ++counts.values["selected_extra_fields"];++counts.layouts[signature+":"+std::to_string(size)];
        }
        result.fields.push_back(object({{"kind",bytes(body,start,start+4)},{"decoded_offset",std::to_string(start)},{"bytes",std::to_string(size)},{"sha256",quote(fallout_tables::hash(data))},{"value",value}}));++counts.values["fields"];
    }
    if(extended||!core_seen[0]||!core_seen[1])throw std::runtime_error("incomplete placed actor fields/core");
    result.core=object({{"base",base_field},{"transform_decoded_offset",std::to_string(transform_offset)},{"position_bits",positions},{"rotation_bits",rotations},{"scale",scale}});return result;
}
static std::string optional(Word value){return value?std::to_string(*value):"null";}
static std::string project(const HeaderIndex& index){
    Counts counts;std::vector<std::string> definitions;
    for(const auto& winner:index.winners){
        const auto& entry=winner.second;const auto& source=index.plugins[entry.source];const std::string kind(entry.header.begin(),entry.header.begin()+4);if(kind!="ACHR"&&kind!="ACRE")continue;
        if(++counts.values["records"]>65536)throw std::runtime_error("placed actor record budget");++counts.kinds[kind];
        const auto flags=static_cast<uint32_t>(integer(entry.header,8,4));const bool deleted=flags&0x20;const auto version=static_cast<uint16_t>(integer(entry.header,20,2));Document document;std::string body_sha="null";
        if(deleted)++counts.values["deleted_records"];
        else{
            auto body=source.source->read(entry.offset+24,static_cast<size_t>(integer(entry.header,4,4)));
            if(flags&0x40000){if(body.size()<4)throw std::runtime_error("compressed placed actor header");const auto expected=static_cast<size_t>(integer(body,0,4));if(expected>64*1024*1024)throw std::runtime_error("compressed placed actor budget");auto decoded=fallout_zlib::decode(Bytes(body.begin()+4,body.end()),expected);if(decoded.stored_adler!=decoded.calculated_adler)throw std::runtime_error("placed actor compressed checksum");body=std::move(decoded.payload);}
            counts.values["decoded_bytes"]+=body.size();if(body.size()>64*1024*1024||counts.values["decoded_bytes"]>256ULL*1024*1024)throw std::runtime_error("placed actor decoded byte budget");
            body_sha=quote(fallout_tables::hash(body));document=decode(index,entry,body,kind,version,counts);++counts.versions[kind+":"+std::to_string(version)];
        }
        const auto header=object({{"kind",bytes(entry.header,0,4)},{"offset",std::to_string(entry.offset)},{"stored_size",std::to_string(integer(entry.header,4,4))},{"flags",std::to_string(flags)},{"form_id",std::to_string(integer(entry.header,12,4))},{"revision",bytes(entry.header,16,20)},{"version",std::to_string(version)},{"trailing_bytes",bytes(entry.header,22,24)}});
        const auto parent=object({{"topic",optional(entry.topic)},{"world",optional(entry.world)},{"cell",optional(entry.cell)},{"child_group",entry.child?std::to_string(static_cast<int32_t>(*entry.child)):"null"}});
        definitions.push_back(object({{"key",winner.first.json()},{"source",object({{"plugin",quote(source.name)},{"sha256",quote(source.source->sha256)},{"record_file_offset",std::to_string(entry.offset)},{"record_flags",std::to_string(flags)},{"decoded_record_sha256",body_sha}})},
            {"header",header},{"parent",parent},{"deleted",deleted?"true":"false"},{"core",document.core},{"base",document.base},{"base_schema_kind_allowed",document.allowed},{"fields",array(document.fields)},{"findings",document.findings_json()}}));
    }return object({{"counts",counts.json()},{"definitions",array(definitions)}});
}
}
