// Original direct-source CLAS projection; no production decoder is invoked.
#pragma once
namespace actor_classes {
using namespace fallout_records;
static std::string bytes(const Bytes& data,size_t begin,size_t end) {
    if(begin>end||end>data.size())throw std::runtime_error("class byte array extent");
    std::vector<std::string> rows;for(size_t at=begin;at<end;++at)rows.push_back(std::to_string(data[at]));return array(rows);
}
static std::string signed_word(uint64_t value,unsigned width) {
    const auto sign=uint64_t(1)<<(width-1);
    return std::to_string(value&sign?static_cast<int64_t>(value)-static_cast<int64_t>(sign*2):static_cast<int64_t>(value));
}
struct Counts {
    std::map<std::string,uint64_t> values{{"records",0},{"deleted_records",0},{"decoded_bytes",0},{"fields",0},{"scalar_fields",0},{"source_findings",0}};
    std::map<std::string,uint64_t> versions,layouts;
    static std::string numbers(const std::map<std::string,uint64_t>& input){Object rows;for(const auto& row:input)rows[row.first]=std::to_string(row.second);return object(rows);}
    std::string json()const{Object rows;for(const auto& row:values)rows[row.first]=std::to_string(row.second);rows["record_versions"]=numbers(versions);rows["scalar_layouts"]=numbers(layouts);return object(rows);}
};
struct Document {std::vector<std::string> fields,findings;};
static Document decode(const Bytes& body,uint16_t version,Counts& counts) {
    if(version!=14&&version!=15)throw std::runtime_error("unsupported CLAS record version");
    Document result;std::optional<size_t> extended;size_t seen[2]{};
    const auto finding=[&](const std::string& offset,const std::string& code){result.findings.push_back(object({{"field_decoded_offset",offset},{"code",quote(code)}}));++counts.values["source_findings"];};
    for(size_t cursor=0;cursor<body.size();) {
        if(body.size()-cursor<6||counts.values["fields"]>=2000000)throw std::runtime_error("class field header/budget");
        const size_t start=cursor;const std::string kind(body.begin()+cursor,body.begin()+cursor+4);
        const auto short_size=static_cast<size_t>(integer(body,cursor+4,2));cursor+=6;
        if(kind=="XXXX") {
            if(short_size!=4||extended)throw std::runtime_error("class extended prefix");
            extended=static_cast<size_t>(integer(body,cursor,4));cursor+=4;continue;
        }
        const auto size=extended.value_or(short_size);extended.reset();
        if(size>body.size()-cursor)throw std::runtime_error("class field extent");
        const Bytes data(body.begin()+cursor,body.begin()+cursor+size);cursor+=size;
        std::string value=object({{"kind",quote("opaque")}});int selected=-1;
        if(kind=="DATA") {
            if(size!=28)throw std::runtime_error("unsupported CLAS DATA extent");selected=0;
            std::vector<std::string> tags;for(size_t at=0;at<16;at+=4)tags.push_back(signed_word(integer(data,at,4),32));
            value=object({{"kind",quote("class_data")},{"tag_skills",array(tags)},{"flags",std::to_string(integer(data,16,4))},
                {"services",std::to_string(integer(data,20,4))},{"teaches",signed_word(data[24],8)},
                {"maximum_training_level",std::to_string(data[25])},{"unused",bytes(data,26,28)}});
        }else if(kind=="ATTR") {
            if(size!=7)throw std::runtime_error("unsupported CLAS ATTR extent");selected=1;
            value=object({{"kind",quote("attributes")},{"attributes",bytes(data,0,7)}});
        }
        if(selected>=0) {
            if(++seen[selected]>1)finding(std::to_string(start),selected==0?"multiple_class_data_fields":"multiple_class_attribute_fields");
            ++counts.values["scalar_fields"];++counts.layouts[kind+":"+std::to_string(size)];
        }
        result.fields.push_back(object({{"kind",bytes(body,start,start+4)},{"decoded_offset",std::to_string(start)},
            {"bytes",std::to_string(size)},{"sha256",quote(fallout_tables::hash(data))},{"value",value}}));++counts.values["fields"];
    }
    if(extended)throw std::runtime_error("orphan class extended prefix");
    if(!seen[0])finding("null","missing_class_data_field");
    if(!seen[1])finding("null","missing_class_attribute_field");return result;
}
static std::string project(const HeaderIndex& index) {
    Counts counts;std::vector<std::string> definitions;
    for(const auto& winner:index.winners) {
        const auto& entry=winner.second;const auto& source=index.plugins[entry.source];
        if(std::string(entry.header.begin(),entry.header.begin()+4)!="CLAS")continue;
        if(++counts.values["records"]>65536)throw std::runtime_error("class record budget");
        const auto flags=static_cast<uint32_t>(integer(entry.header,8,4));const bool deleted=flags&0x20;
        const auto version=static_cast<uint16_t>(integer(entry.header,20,2));Document document;std::string body_sha="null";
        if(deleted)++counts.values["deleted_records"];
        else {
            auto body=source.source->read(entry.offset+24,static_cast<size_t>(integer(entry.header,4,4)));
            if(flags&0x40000) {
                if(body.size()<4)throw std::runtime_error("compressed class header");
                const auto expected=static_cast<size_t>(integer(body,0,4));if(expected>64*1024*1024)throw std::runtime_error("compressed class budget");
                auto decoded=fallout_zlib::decode(Bytes(body.begin()+4,body.end()),expected);
                if(decoded.stored_adler!=decoded.calculated_adler)throw std::runtime_error("class compressed checksum");body=std::move(decoded.payload);
            }
            counts.values["decoded_bytes"]+=body.size();
            if(body.size()>64*1024*1024||counts.values["decoded_bytes"]>256ULL*1024*1024)throw std::runtime_error("class decoded byte budget");
            body_sha=quote(fallout_tables::hash(body));document=decode(body,version,counts);++counts.versions[std::to_string(version)];
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
