// Original direct-source CTDA word/dependency comparison. No native handler or
// Rust parser runs here. Compressed bytes use the separate verified RFC reader.
#include "../oracle-common/record_source.hpp"
#include "../oracle-common/pe_source.hpp"
#include "../oracle-common/zlib_source.hpp"
using namespace fallout_records;

static std::string hash(const std::string& value) {
    return fallout_tables::hash(Bytes(value.begin(),value.end()));
}
static int64_t signed_word(uint32_t word) { return word & 0x80000000U ? int64_t(word)-0x100000000LL : word; }
static std::string optional_word(Word word) { return word ? std::to_string(*word) : "null"; }
struct Counts {
    std::map<std::string,uint64_t> numbers;
    std::map<std::string,std::map<std::string,uint64_t>> maps;
    Counts() {
        for(const auto key:{"candidate_records","deleted_candidate_records","decoded_candidate_bytes","records_with_conditions","conditions",
            "source_findings","unused_nonzero_words","unknown_parameters","variable_indices_need_live_state"})numbers[key]=0;
        for(const auto key:{"signature_statuses","parameter_kinds","parameter_domains","subjects","form_statuses"})maps[key]={};
    }
    std::string json()const {
        Object out;for(const auto& entry:numbers)out[entry.first]=std::to_string(entry.second);
        for(const auto& map:maps){Object rows;for(const auto& entry:map.second)rows[entry.first]=std::to_string(entry.second);out[map.first]=object(rows);}
        return object(out);
    }
};
struct Parameter { uint32_t type,optional; };
using Signature=std::vector<Parameter>;
static std::map<uint16_t,Signature> signatures(const fallout_oracle::SourceImage& image) {
    std::map<uint16_t,Signature> result;
    for(uint16_t index=0;index<640;++index) {
        const uint32_t address=0x01190910+index*40;
        if(!image.number(address+32,4))continue;
        const auto count=image.number(address+18,2),pointer=image.number(address+20,4);
        if(count>64)throw std::runtime_error("condition descriptor parameter budget");
        Signature parameters;
        for(uint32_t i=0;i<count;++i)parameters.push_back({image.number(pointer+i*12+4,4),image.number(pointer+i*12+8,4)});
        result[index]=parameters;
    }return result;
}
struct Value {
    std::string kind,domain;uint32_t word;
    std::string json()const {
        Object out{{"kind",quote(kind)},{"raw_word",std::to_string(word)}};
        if(!domain.empty())out["domain"]=quote(domain);
        if(kind=="signed_integer"||kind=="signed_domain")out["value"]=std::to_string(signed_word(word));
        if(kind=="variable_index")out["signed_index"]=std::to_string(signed_word(word));
        return object(out);
    }
};
static Value word_value(uint16_t function,size_t index,uint32_t word,uint32_t type,uint32_t first) {
    if(function==408) {
        if(type!=1)return {"unknown","",word};
        if(!index)return {"unsigned_domain","vats_function",word};
        const std::map<uint32_t,std::pair<std::string,std::string>> choices{
            {0,{"form_id","weapon"}},{1,{"form_id","form_list"}},{2,{"form_id","actor_base"}},{3,{"form_id","form_list"}},
            {5,{"signed_domain","actor_value"}},{6,{"unsigned_domain","vats_action"}},{9,{"form_id","effect_item"}},
            {10,{"form_id","form_list"}},{15,{"unsigned_domain","weapon_type"}}};
        const auto found=choices.find(first);
        if(found!=choices.end())return {found->second.first,found->second.second,word};
        if(first<=17)return {"unused","",word};return {"unknown","",word};
    }
    if(function==427)return type==46?Value{"form_id","voice_type",word}:Value{"unknown","",word};
    if(type==1) {
        if((function==420||function==421)&&index==1)return {"signed_domain","quest_objective",word};
        if(function==36&&!index)return {"unsigned_domain","menu_mode",word};
        if(function==438&&!index)return {"unsigned_domain","creature_type",word};
        if(function==398&&!index)return {"signed_domain","body_location",word};
        return {"signed_integer","",word};
    }
    if(type==2)return {"float_bits","",word};
    if(type==22)return {"variable_index","",word};
    const std::map<uint32_t,std::pair<std::string,std::string>> numbers{
        {5,{"signed_domain","actor_value"}},{8,{"unsigned_domain","axis"}},{18,{"unsigned_domain","sex"}},
        {23,{"signed_domain","quest_stage"}},{28,{"unsigned_domain","crime_type"}},{32,{"unsigned_domain","form_type"}},
        {41,{"unsigned_domain","misc_stat"}},{51,{"unsigned_domain","alignment"}},{52,{"unsigned_domain","equip_type"}},
        {55,{"unsigned_domain","critical_stage"}}};
    const auto number=numbers.find(type);if(number!=numbers.end())return {number->second.first,number->second.second,word};
    const std::map<uint32_t,std::string> forms{{3,"inventory_object"},{50,"inventory_object"},{4,"reference"},{6,"actor"},
        {9,"cell"},{11,"effect_item"},{14,"quest"},{15,"race"},{16,"class"},{17,"faction"},{19,"global"},{20,"furniture"},
        {21,"base_object"},{53,"base_object"},{25,"actor_base"},{27,"worldspace"},{29,"package"},{31,"base_effect"},
        {33,"weather"},{35,"owner"},{37,"form_list"},{39,"perk"},{40,"note"},{47,"encounter_zone"},{48,"idle"},
        {61,"form"},{62,"reputation"},{63,"casino"},{65,"challenge"},{69,"region"}};
    const auto form=forms.find(type);if(form!=forms.end())return {"form_id",form->second,word};return {"unknown","",word};
}
static std::string dependency(const HeaderIndex& index,const Plugin& source,uint32_t raw,Counts& counts) {
    const auto key=resolve(source.name,source.masters,raw);std::string status="null",binding="null",target="null";
    if(key) {
        const auto found=index.winners.find(*key);
        if(found!=index.winners.end()) {
            const auto& entry=found->second;const auto flags=integer(entry.header,8,4);
            status=flags&0x20?"deleted":"defined";
            target=object({{"source_name",quote(index.plugins[entry.source].name)},
                {"record_kind",quote(std::string(entry.header.begin(),entry.header.begin()+4))},
                {"record_file_offset",std::to_string(entry.offset)},{"record_flags",std::to_string(flags)}});
        } else if(key->origin=="falloutnv.esm"&&key->local==0x14) { status="runtime_dependency";binding=quote("nv-player-reference"); }
        else status="missing";
    }
    ++counts.maps["form_statuses"][status];
    return object({{"raw_word",std::to_string(raw)},{"key",key?key->json():"null"},{"status",quote(status)},
        {"runtime_binding",binding},{"target",target}});
}
struct Bound { std::string json,subject; };
static Bound bind(const HeaderIndex& index,const Plugin& source,const Bytes& data,
    const std::map<uint16_t,Signature>& registry,Counts& counts) {
    const auto function=static_cast<uint16_t>(integer(data,8,2));const auto found=registry.find(function);
    const Signature* signature=found==registry.end()?nullptr:&found->second;
    std::string status="descriptor_bound";
    if(!signature)status="missing_descriptor";
    else if(std::any_of(signature->begin(),signature->end(),[](const Parameter& p){return p.optional>1;}))status="unverified_optional_word";
    else if(signature->size()>2)status="additional_parameters";
    const auto first=static_cast<uint32_t>(integer(data,12,4));std::vector<std::string> operands;bool unknown=false;
    for(size_t i=0;i<2;++i) {
        const auto raw=static_cast<uint32_t>(integer(data,12+i*4,4));const auto parameter=signature&&i<signature->size()?&signature->at(i):nullptr;
        Value value{"unknown","",raw};
        if(status=="descriptor_bound")value=parameter?word_value(function,i,raw,parameter->type,first):Value{"unused","",raw};
        ++counts.maps["parameter_kinds"][value.kind];if(!value.domain.empty())++counts.maps["parameter_domains"][value.domain];
        counts.numbers["unused_nonzero_words"]+=value.kind=="unused"&&raw;
        counts.numbers["unknown_parameters"]+=value.kind=="unknown";
        counts.numbers["variable_indices_need_live_state"]+=value.kind=="variable_index";
        unknown|=value.kind=="unknown";
        operands.push_back(object({{"parameter_type_id",parameter?std::to_string(parameter->type):"null"},
            {"optional_word",parameter?std::to_string(parameter->optional):"null"},{"value",value.json()},
            {"form_dependency",value.kind=="form_id"?dependency(index,source,raw,counts):"null"}}));
    }
    if(status=="descriptor_bound"&&unknown)status="schema_disagreement";++counts.maps["signature_statuses"][status];
    const Word run=data.size()>=24?Word(static_cast<uint32_t>(integer(data,20,4))):Word{};
    const Word reference=data.size()>=28?Word(static_cast<uint32_t>(integer(data,24,4))):Word{};
    std::string subject="absent";Object subject_fields;
    if(function==106||function==285){subject="animation_group";subject_fields["raw_word"]=optional_word(run);}
    else if(run) {
        const char* choices[]{"subject","target","reference","combat_target","linked_reference"};
        subject=*run<5?choices[*run]:"unknown";
        if(subject=="reference")subject_fields["raw_word"]=optional_word(reference);
        if(subject=="unknown")subject_fields["raw_word"]=std::to_string(*run);
    }
    subject_fields["kind"]=quote(subject);++counts.maps["subjects"][subject];
    return {object({{"signature_status",quote(status)},{"signature_parameter_count",signature?std::to_string(signature->size()):"null"},
        {"operands",array(operands)},{"comparison_global",data[0]&4?dependency(index,source,static_cast<uint32_t>(integer(data,4,4)),counts):"null"},
        {"subject",object(subject_fields)},{"subject_reference",subject=="reference"&&reference?dependency(index,source,*reference,counts):"null"},
        {"legacy_target_flag_present",data[0]&2?"true":"false"},{"live_values_resolved","false"},{"evaluation_ready","false"}}),subject};
}
static bool condition_kind(const std::string& kind) {
    const std::set<std::string> kinds{"ALCH","ENCH","INGR","SPEL","TERM","PERK","CPTH","MESG","IDLE","INFO","PACK","QUST","RCPE"};
    return kinds.count(kind)!=0;
}
static std::vector<std::string> condition_rows(const HeaderIndex& index,const Plugin& source,const Bytes& payload,
    const std::map<uint16_t,Signature>& registry,Counts& counts) {
    std::vector<std::string> rows;std::optional<size_t> extended,previous_offset;std::optional<std::string> previous_kind;size_t fields=0;
    for(size_t cursor=0;cursor<payload.size();) {
        if(++fields>1048576||payload.size()-cursor<6)throw std::runtime_error("condition field header/budget");
        const size_t offset=cursor;const std::string kind(payload.begin()+cursor,payload.begin()+cursor+4);
        const auto short_size=static_cast<size_t>(integer(payload,cursor+4,2));cursor+=6;
        if(kind=="XXXX") {
            if(extended||short_size!=4)throw std::runtime_error("condition XXXX shape");
            extended=static_cast<size_t>(integer(payload,cursor,4));cursor+=4;continue;
        }
        const auto length=extended.value_or(short_size);extended.reset();
        if(length>payload.size()-cursor)throw std::runtime_error("condition field extent");
        if(kind=="CTDA") {
            if(length!=20&&length!=24&&length!=28)throw std::runtime_error("condition CTDA extent");
            if(++counts.numbers["conditions"]>1000000)throw std::runtime_error("condition row budget");
            const Bytes data(payload.begin()+cursor,payload.begin()+cursor+length);
            const auto bound=bind(index,source,data,registry,counts);std::vector<std::string> findings;
            if(data[0]&0x1a)findings.push_back(quote("uninterpreted_low_flags"));
            if(data[1]||data[2]||data[3])findings.push_back(quote("nonzero_flag_padding"));
            if(data[10]||data[11])findings.push_back(quote("nonzero_function_padding"));
            const auto comparison=integer(data,4,4);const auto operation=data[0]>>5;
            if(operation>5)findings.push_back(quote("unknown_comparison_operator"));
            if(!(data[0]&4)&&(comparison&0x7f800000)==0x7f800000)findings.push_back(quote("nonfinite_comparison"));
            if(bound.subject=="unknown")findings.push_back(quote("unknown_subject_selector"));
            counts.numbers["source_findings"]+=findings.size();
            const char* operations[]{"equal","not_equal","greater","greater_or_equal","less","less_or_equal"};
            rows.push_back(object({{"field_decoded_offset",std::to_string(offset)},{"bytes",std::to_string(length)},
                {"sha256",quote(fallout_tables::hash(data))},{"preceding_field_kind",previous_kind?quote(*previous_kind):"null"},
                {"preceding_field_decoded_offset",previous_offset?std::to_string(*previous_offset):"null"},{"flags",std::to_string(data[0])},
                {"flag_padding",array({std::to_string(data[1]),std::to_string(data[2]),std::to_string(data[3])})},
                {"function_id",std::to_string(integer(data,8,2))},{"function_padding",array({std::to_string(data[10]),std::to_string(data[11])})},
                {"comparison_operator",operation<6?quote(operations[operation]):object({{"unknown",std::to_string(operation)}})},
                {"comparison_value",object({{data[0]&4?"global_raw_form":"float_bits",std::to_string(comparison)}})},
                {"or_flag",data[0]&1?"true":"false"},{"parameter_words",array({std::to_string(integer(data,12,4)),std::to_string(integer(data,16,4))})},
                {"run_on_word",length>=24?std::to_string(integer(data,20,4)):"null"},{"reference_word",length>=28?std::to_string(integer(data,24,4)):"null"},
                {"binding",bound.json},{"source_findings",array(findings)}}));
        }
        previous_kind=kind;previous_offset=offset;cursor+=length;
    }
    if(extended)throw std::runtime_error("orphan condition XXXX");return rows;
}
int wmain(int argc,wchar_t** argv) {
    try {
        if(argc!=4)throw std::runtime_error("usage: condition-operand-oracle Data_directory FRORDER1_bundle FalloutNV.exe");
        auto index=scan(argv[1],argv[2]);Source executable_guard(argv[3]);const fallout_oracle::SourceImage image(argv[3]);
        if(executable_guard.sha256!=image.sha256)throw std::runtime_error("executable read cohort");
        const auto registry=signatures(image);Counts counts;std::vector<std::string> records;
        for(const auto& item:index.winners) {
            const auto& entry=item.second;const std::string kind(entry.header.begin(),entry.header.begin()+4);
            if(!condition_kind(kind))continue;
            if(++counts.numbers["candidate_records"]>262144)throw std::runtime_error("condition candidate budget");
            const auto flags=integer(entry.header,8,4);if(flags&0x20){++counts.numbers["deleted_candidate_records"];continue;}
            auto& source=index.plugins[entry.source];const auto stored=source.source->read(entry.offset+24,static_cast<size_t>(integer(entry.header,4,4)));
            Bytes payload=stored;
            if(flags&0x40000) {
                const auto expected=static_cast<size_t>(integer(stored,0,4));const Bytes zlib(stored.begin()+4,stored.end());
                auto decoded=fallout_zlib::decode(zlib,expected);
                if(decoded.stored_adler!=decoded.calculated_adler)throw std::runtime_error("condition checksum mismatch");
                payload=std::move(decoded.payload);
            }
            counts.numbers["decoded_candidate_bytes"]+=payload.size();
            if(counts.numbers["decoded_candidate_bytes"]>512ULL*1024*1024)throw std::runtime_error("condition decoded budget");
            const auto rows=condition_rows(index,source,payload,registry,counts);
            if(rows.empty())continue;++counts.numbers["records_with_conditions"];
            records.push_back(object({{"key",item.first.json()},{"source_name",quote(source.name)},{"record_kind",quote(kind)},
                {"record_file_offset",std::to_string(entry.offset)},{"record_flags",std::to_string(flags)},{"decoded_bytes",std::to_string(payload.size())},
                {"decoded_sha256",quote(fallout_tables::hash(payload))},{"binding_sha256",quote(hash(array(rows)))},{"conditions",array(rows)}}));
        }
        std::cout<<object({{"schema_version","1"},{"profile",quote("nv-original")},{"metadata",index.metadata_json()},{"sources",index.sources_json()},
            {"executable_sha256",quote(image.sha256)},{"counts",counts.json()},{"records",array(records)},
            {"target_kind_acceptance_checked","false"},{"live_values_resolved","false"},{"evaluation_ready","false"},{"retail_parity_accepted","false"}})<<'\n';
        return counts.numbers["source_findings"]||counts.numbers["unknown_parameters"]?1:0;
    }catch(const std::exception& error){std::cerr<<"condition-operand-oracle: "<<error.what()<<'\n';return 1;}
}
